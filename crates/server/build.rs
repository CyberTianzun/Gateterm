//! 编译前先构建 web 客户端:assets.rs 用 rust-embed 把 clients/web/dist/ 编进二进制,
//! 不在这里构建的话,改了前端只跑 cargo build 会拿到旧页面。
//!
//! - 只在 clients/web 的源码/配置变化时重跑(cargo 按 rerun-if-changed 的 mtime 判断)
//! - 找不到 npm 时只警告并沿用现有 dist,不让没装 Node 的环境编不过
//! - 设置 OCTOTERM_SKIP_WEB_BUILD=1 可跳过(例如前端已由外部步骤构建好)

use std::path::Path;
use std::process::Command;

fn main() {
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../clients/web");

    println!("cargo:rerun-if-env-changed=OCTOTERM_SKIP_WEB_BUILD");
    for p in ["src", "scripts", "package.json", "package-lock.json", "tsconfig.json"] {
        println!("cargo:rerun-if-changed={}", web.join(p).display());
    }

    if std::env::var_os("OCTOTERM_SKIP_WEB_BUILD").is_some_and(|v| v != "0") {
        return;
    }

    // Windows 上 npm 是 npm.cmd,Command 不会自动补扩展名
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };

    if !web.join("node_modules").exists() && !run(npm, &["install"], &web) {
        return;
    }
    run(npm, &["run", "build"], &web);
}

/// 返回 false 表示 npm 不存在(已警告);npm 本身执行失败直接让构建失败
fn run(npm: &str, args: &[&str], dir: &Path) -> bool {
    match Command::new(npm).args(args).current_dir(dir).status() {
        Ok(s) if s.success() => true,
        Ok(s) => panic!("`npm {}` 失败({s}),见上方输出", args.join(" ")),
        Err(e) => {
            println!("cargo:warning=找不到 npm({e}),跳过 web 构建,沿用现有 clients/web/dist/");
            false
        }
    }
}
