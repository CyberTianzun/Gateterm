//! 问 Codex 自己:我们的 hook 被它放行了没有。
//!
//! Codex 按 `trusted_hash` 逐条门控 hook,没审核过(或审核后内容变了)的一律**静默跳过**
//! —— 不报错、不提示,gateterm 这边就是什么都收不到。哈希怎么算是 Codex 的内部细节,
//! 我们不复算;它的 app-server 协议里有一个只读的 `hooks/list`,每条 hook 直接带
//! `trustStatus`(`trusted` / `managed` / `untrusted` / `modified`),这是唯一权威的答案。
//!
//! 做法:起一个 `codex app-server`(stdio JSON-RPC),`initialize` → `hooks/list` → 杀掉。
//! 实测几十毫秒。**不碰任何状态**:不写信任、不开会话、不触发 hook。
//!
//! 任何一步失败(没装 codex、协议变了、超时)都返回 `None` —— 由调用方回落到读
//! config.toml 的粗判断,而不是把一个查不到的状态报给用户。

use serde_json::{json, Value};
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::edit::is_ours;

const TIMEOUT: Duration = Duration::from_secs(5);
const LIST_ID: u64 = 2;

/// `Some(true)` = 我们在 `hooks_file` 里的每一条 hook 都已放行;`Some(false)` = 至少一条
/// 没有;`None` = 问不出来。
pub fn query(exe: &Path, path_env: &OsStr, codex_home: &Path, hooks_file: &Path, port: u16) -> Option<bool> {
    let mut child = Command::new(exe)
        .arg("app-server")
        .env("PATH", path_env)
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let reply = exchange(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    verdict(&reply?, hooks_file, port)
}

fn exchange(child: &mut std::process::Child) -> Option<Value> {
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let requests = [
        json!({ "id": 1, "method": "initialize",
                "params": { "clientInfo": { "name": "gateterm", "title": null, "version": env!("CARGO_PKG_VERSION") },
                            "capabilities": null } }),
        json!({ "method": "initialized" }),
        // cwd 只影响项目级 hook 的发现;我们装的是用户级的,给根目录即可
        json!({ "id": LIST_ID, "method": "hooks/list", "params": { "cwds": ["/"] } }),
    ];
    for r in requests {
        writeln!(stdin, "{r}").ok()?;
    }
    stdin.flush().ok()?;

    // 读放到线程里,好让超时由这边说了算 —— 卡住的 codex 不能拖住 `GET /api/agents`
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            if msg.get("id").and_then(Value::as_u64) == Some(LIST_ID) {
                let _ = tx.send(msg);
                break;
            }
        }
    });
    let deadline = Instant::now() + TIMEOUT;
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok()
}

/// 只看来自 `hooks_file`、且是我们当前端口的那些条目。一条都没有 = 问不出来
/// (说明它看到的和我们读到的不是同一份文件),不下结论。
pub fn verdict(reply: &Value, hooks_file: &Path, port: u16) -> Option<bool> {
    let entries = reply.get("result")?.get("data")?.as_array()?;
    let file = hooks_file.to_string_lossy();
    let statuses: Vec<&str> = entries
        .iter()
        .filter_map(|e| e.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter(|h| h.get("sourcePath").and_then(Value::as_str) == Some(&*file))
        .filter(|h| {
            let cmd = h.get("command").and_then(Value::as_str).unwrap_or_default();
            is_ours(&json!({ "type": "command", "command": cmd }), port)
        })
        .map(|h| h.get("trustStatus").and_then(Value::as_str).unwrap_or(""))
        .collect();
    if statuses.is_empty() {
        return None;
    }
    Some(statuses.iter().all(|s| matches!(*s, "trusted" | "managed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(path: &str, port: u16, status: &str) -> Value {
        json!({
            "sourcePath": path,
            "handlerType": "command",
            "command": format!("\"/bin/gateterm-server\" hook http://127.0.0.1:{port}/hook/codex/stop"),
            "trustStatus": status,
        })
    }

    fn reply(hooks: Vec<Value>) -> Value {
        json!({ "id": 2, "result": { "data": [{ "cwd": "/", "hooks": hooks, "warnings": [], "errors": [] }] } })
    }

    const FILE: &str = "/h/.codex/hooks.json";

    #[test]
    fn all_trusted() {
        let r = reply(vec![hook(FILE, 7683, "trusted"), hook(FILE, 7683, "trusted")]);
        assert_eq!(verdict(&r, Path::new(FILE), 7683), Some(true));
    }

    #[test]
    fn one_modified_is_not_activated() {
        let r = reply(vec![hook(FILE, 7683, "trusted"), hook(FILE, 7683, "modified")]);
        assert_eq!(verdict(&r, Path::new(FILE), 7683), Some(false));
    }

    /// 别人的 hook、别的文件、旧端口上的残留都不算 —— 它们的信任状态和我们无关。
    #[test]
    fn ignores_foreign_entries() {
        let mut foreign = hook(FILE, 7683, "untrusted");
        foreign["command"] = json!("echo hi");
        let r = reply(vec![
            hook(FILE, 7683, "trusted"),
            foreign,
            hook("/proj/.codex/hooks.json", 7683, "untrusted"),
            hook(FILE, 1111, "untrusted"),
        ]);
        assert_eq!(verdict(&r, Path::new(FILE), 7683), Some(true));
    }

    #[test]
    fn nothing_of_ours_is_unknown() {
        assert_eq!(verdict(&reply(vec![]), Path::new(FILE), 7683), None);
        assert_eq!(verdict(&json!({ "id": 2, "error": {} }), Path::new(FILE), 7683), None);
    }
}
