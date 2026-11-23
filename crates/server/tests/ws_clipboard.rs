mod common;

use common::{connect, control, create_session, input_frame, parse_server, start_test_server, Ws};
use futures_util::{SinkExt, StreamExt};
use gateterm_protocol::{AttachMode, ClientMsg, ServerMsg};
use std::time::Duration;

/// 每读到一行 base64 词,就输出 `BEGIN_<b64><OSC 52><END_<b64>>` —— 既验证 OSC 52
/// 被从字节流里干净剥掉,又验证解出的文本只发给刚刚输入的那一端。
fn osc52_echo_cmd() -> Option<Vec<String>> {
    #[cfg(unix)]
    return Some(vec![
        "/bin/sh".into(),
        "-c".into(),
        r#"while IFS= read -r b; do printf 'BEGIN_%s\033]52;c;%s\007END_%s\n' "$b" "$b" "$b"; done"#
            .into(),
    ]);
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        Some(vec![
            format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"),
            "-NoProfile".into(),
            "-NoLogo".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            "while (($b = [Console]::In.ReadLine()) -ne $null) { \
             [Console]::Out.Write('BEGIN_' + $b + [char]27 + ']52;c;' + $b + [char]7 + 'END_' + $b + \"`n\") }"
                .into(),
        ])
    }
}

async fn attach_and_wait_resync(ws: &mut Ws, id: u64, channel: u32) {
    ws.send(control(&ClientMsg::Attach {
        id,
        channel,
        last_seq: None,
        cols: 80,
        rows: 24,
        colors: None,
    }))
    .await
    .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if let Some((0, Ok(ServerMsg::ResyncEnd { channel: ch, .. }))) =
            parse_server(ws.next().await.unwrap().unwrap())
            && ch == channel
        {
            return;
        }
    }
    panic!("timed out waiting for ResyncEnd on channel {channel}");
}

/// 收集 channel 上的字节直到出现 `needle`,同时记录期间收到的所有 `ServerMsg::Clipboard`。
async fn collect_until(ws: &mut Ws, channel: u32, needle: &str) -> (String, Vec<(u32, String)>) {
    let mut bytes_acc = Vec::new();
    let mut clips = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(250), ws.next()).await
        else {
            continue;
        };
        match parse_server(msg) {
            Some((0, Ok(ServerMsg::Clipboard { channel: ch, text }))) => clips.push((ch, text)),
            Some((ch, Err(data))) if ch == channel => {
                bytes_acc.extend_from_slice(&data);
                if String::from_utf8_lossy(&bytes_acc).contains(needle) {
                    // 再短暂排空一次控制帧,确保紧随数据之后的 clipboard(若有)也被收进来
                    while let Ok(Some(Ok(extra))) =
                        tokio::time::timeout(Duration::from_millis(80), ws.next()).await
                    {
                        match parse_server(extra) {
                            Some((0, Ok(ServerMsg::Clipboard { channel: ch, text }))) => {
                                clips.push((ch, text));
                            }
                            Some((ch, Err(more))) if ch == channel => {
                                bytes_acc.extend_from_slice(&more);
                            }
                            _ => {}
                        }
                    }
                    return (String::from_utf8_lossy(&bytes_acc).into_owned(), clips);
                }
            }
            _ => {}
        }
    }
    panic!(
        "timed out waiting for {needle:?}; got {:?}, clips={clips:?}",
        String::from_utf8_lossy(&bytes_acc)
    );
}

#[tokio::test]
async fn osc52_routes_only_to_input_owner_and_never_broadcasts_or_replays() {
    let url = start_test_server("t").await;
    let mut ws_a = connect(&url).await;
    let mut ws_b = connect(&url).await;
    let id = create_session(&mut ws_a, osc52_echo_cmd()).await;

    attach_and_wait_resync(&mut ws_a, id, 1).await;
    attach_and_wait_resync(&mut ws_b, id, 2).await;

    // 1. 客户端 B 发输入触发复制("from-b" -> base64 "ZnJvbS1i"):只有 B 收到 clipboard,A 不收
    ws_b.send(input_frame(2, b"ZnJvbS1i\r")).await.unwrap();
    let (out_b1, clips_b1) = collect_until(&mut ws_b, 2, "END_ZnJvbS1i").await;
    let (out_a1, clips_a1) = collect_until(&mut ws_a, 1, "END_ZnJvbS1i").await;
    assert!(out_b1.contains("BEGIN_ZnJvbS1iEND_ZnJvbS1i"), "OSC 52 must be stripped: {out_b1:?}");
    assert!(!out_b1.contains("]52;"), "raw OSC 52 leaked to B: {out_b1:?}");
    assert!(!out_a1.contains("]52;"), "raw OSC 52 leaked to A: {out_a1:?}");
    assert_eq!(clips_b1, vec![(2, "from-b".to_string())]);
    assert!(clips_a1.is_empty(), "A must not receive B's clipboard: {clips_a1:?}");

    // 2. 换客户端 A 操作("from-a" -> base64 "ZnJvbS1h"):这次只有 A 收到,B 不收
    ws_a.send(input_frame(1, b"ZnJvbS1h\r")).await.unwrap();
    let (out_a2, clips_a2) = collect_until(&mut ws_a, 1, "END_ZnJvbS1h").await;
    let (out_b2, clips_b2) = collect_until(&mut ws_b, 2, "END_ZnJvbS1h").await;
    assert!(out_a2.contains("BEGIN_ZnJvbS1hEND_ZnJvbS1h"), "OSC 52 must be stripped: {out_a2:?}");
    assert!(!out_b2.contains("]52;"), "raw OSC 52 leaked to B: {out_b2:?}");
    assert_eq!(clips_a2, vec![(1, "from-a".to_string())]);
    assert!(clips_b2.is_empty(), "B must not receive A's clipboard: {clips_b2:?}");

    // 3. 新客户端 C 从 seq=0 请求 replay:拿到干净的历史输出,既没有 OSC 52 也没有 clipboard
    let mut ws_c = connect(&url).await;
    ws_c.send(control(&ClientMsg::Attach {
        id,
        channel: 5,
        last_seq: Some(0),
        cols: 80,
        rows: 24,
        colors: None,
    }))
    .await
    .unwrap();
    let attached = common::next_control(&mut ws_c).await;
    assert!(
        matches!(attached, ServerMsg::Attached { channel: 5, mode: AttachMode::Replay, .. }),
        "expected Replay, got {attached:?}"
    );
    let (replay_out, clips_c) = collect_until(&mut ws_c, 5, "END_ZnJvbS1h").await;
    assert!(replay_out.contains("BEGIN_ZnJvbS1iEND_ZnJvbS1i"), "{replay_out:?}");
    assert!(replay_out.contains("BEGIN_ZnJvbS1hEND_ZnJvbS1h"), "{replay_out:?}");
    assert!(!replay_out.contains("]52;"), "replay must not contain OSC 52: {replay_out:?}");
    assert!(clips_c.is_empty(), "replay must not re-fire clipboard: {clips_c:?}");

    ws_a.send(control(&ClientMsg::KillSession { id })).await.unwrap();
}

fn osc5522_echo_cmd() -> Option<Vec<String>> {
    #[cfg(unix)]
    return Some(vec![
        "/bin/sh".into(),
        "-c".into(),
        r#"stty -echo; while IFS= read -r line; do case "$line" in *COPY:*) b=$(printf '%s' "${line##*COPY:}" | tr -d '\r'); printf 'BEGIN_%s\033]5522;type=write:id=k1\033\\\033]5522;type=wdata:mime=dGV4dC9wbGFpbg==;%s\033\\\033]5522;type=wdata\033\\END_%s\n' "$b" "$b" "$b";; esac; done"#
            .into(),
    ]);
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        Some(vec![
            format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"),
            "-NoProfile".into(),
            "-NoLogo".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            "while (($line = [Console]::In.ReadLine()) -ne $null) { \
             $idx = $line.LastIndexOf('COPY:'); \
             if ($idx -ge 0) { \
               $b = $line.Substring($idx + 5).Trim(); \
               $e = [char]27; \
               [Console]::Out.Write('BEGIN_' + $b + $e + ']5522;type=write:id=k1' + $e + '\\' + $e + ']5522;type=wdata:mime=dGV4dC9wbGFpbg==;' + $b + $e + '\\' + $e + ']5522;type=wdata' + $e + '\\END_' + $b + \"`n\") \
             } }"
                .into(),
        ])
    }
}

#[tokio::test]
async fn osc5522_strips_from_stream_and_routes_to_input_owner() {
    let url = start_test_server("t").await;
    let mut ws_a = connect(&url).await;
    let mut ws_b = connect(&url).await;
    let id = create_session(&mut ws_a, osc5522_echo_cmd()).await;

    attach_and_wait_resync(&mut ws_a, id, 1).await;
    attach_and_wait_resync(&mut ws_b, id, 2).await;

    // 1. Client B sends input triggering OSC 5522 ("from-b" -> "ZnJvbS1i"): only B receives clipboard
    ws_b.send(input_frame(2, b"COPY:ZnJvbS1i\r")).await.unwrap();
    let (out_b1, clips_b1) = collect_until(&mut ws_b, 2, "END_ZnJvbS1i").await;
    let (out_a1, clips_a1) = collect_until(&mut ws_a, 1, "END_ZnJvbS1i").await;
    assert!(
        out_b1.contains("BEGIN_ZnJvbS1iEND_ZnJvbS1i"),
        "OSC 5522 must be stripped: {out_b1:?}"
    );
    assert!(!out_b1.contains("]5522;"), "raw OSC 5522 leaked to B: {out_b1:?}");
    assert!(!out_a1.contains("]5522;"), "raw OSC 5522 leaked to A: {out_a1:?}");
    assert_eq!(clips_b1, vec![(2, "from-b".to_string())]);
    assert!(clips_a1.is_empty(), "A must not receive B's clipboard: {clips_a1:?}");

    // 2. Client A sends input triggering OSC 5522 ("from-a" -> "ZnJvbS1h"): only A receives clipboard
    ws_a.send(input_frame(1, b"COPY:ZnJvbS1h\r")).await.unwrap();
    let (out_a2, clips_a2) = collect_until(&mut ws_a, 1, "END_ZnJvbS1h").await;
    let (out_b2, clips_b2) = collect_until(&mut ws_b, 2, "END_ZnJvbS1h").await;
    assert!(
        out_a2.contains("BEGIN_ZnJvbS1hEND_ZnJvbS1h"),
        "OSC 5522 must be stripped: {out_a2:?}"
    );
    assert!(!out_b2.contains("]5522;"), "raw OSC 5522 leaked to B: {out_b2:?}");
    assert_eq!(clips_a2, vec![(1, "from-a".to_string())]);
    assert!(clips_b2.is_empty(), "B must not receive A's clipboard: {clips_b2:?}");

    // 3. New client C replaying from seq=0 gets clean output without OSC 5522 or clipboard events
    let mut ws_c = connect(&url).await;
    ws_c.send(control(&ClientMsg::Attach {
        id,
        channel: 5,
        last_seq: Some(0),
        cols: 80,
        rows: 24,
        colors: None,
    }))
    .await
    .unwrap();
    let attached = common::next_control(&mut ws_c).await;
    assert!(
        matches!(attached, ServerMsg::Attached { channel: 5, mode: AttachMode::Replay, .. }),
        "expected Replay, got {attached:?}"
    );
    let (replay_out, clips_c) = collect_until(&mut ws_c, 5, "END_ZnJvbS1h").await;
    assert!(replay_out.contains("BEGIN_ZnJvbS1iEND_ZnJvbS1i"), "{replay_out:?}");
    assert!(replay_out.contains("BEGIN_ZnJvbS1hEND_ZnJvbS1h"), "{replay_out:?}");
    assert!(!replay_out.contains("]5522;"), "replay must not contain OSC 5522: {replay_out:?}");
    assert!(clips_c.is_empty(), "replay must not re-fire clipboard: {clips_c:?}");

    ws_a.send(control(&ClientMsg::KillSession { id })).await.unwrap();
}
