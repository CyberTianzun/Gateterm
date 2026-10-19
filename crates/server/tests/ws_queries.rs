#![cfg(unix)]

mod common;

use common::{connect, control, parse_server};
use futures_util::{SinkExt, StreamExt};
use octoterm_protocol::{ClientMsg, Frame, ServerMsg, TermColors};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

/// 程序问背景色,应答由服务端按 attach 报来的颜色直接写回 pty(协议 D7/D8),
/// 不经过客户端。客户端这边什么都不回,程序照样能读到应答。
#[tokio::test]
async fn background_query_is_answered_with_attach_colors() {
    // 先等一行输入:保证 attach(连同颜色)已经到了服务端再发问。
    // 应答 `ESC ]11;rgb:1919/1a1a/1b1b BEL` 正好 24 字节;ESC/BEL 换成 E/B 方便断言。
    let script = r#"read go; stty raw -echo; printf '\033]11;?\007'
r=$(dd bs=1 count=24 2>/dev/null | tr '\033\007' 'EB'); stty sane
printf '\nREPLY[%s]\n' "$r"; sleep 5"#;
    let url = common::start_test_server("t").await;
    let mut ws = connect(&url).await;
    ws.send(control(&ClientMsg::NewSession {
        name: None,
        command: Some(vec!["sh".into(), "-c".into(), script.into()]),
        cwd: None,
    }))
    .await
    .unwrap();
    let id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some((0, Ok(ServerMsg::SessionEvent { session, .. }))) =
                parse_server(ws.next().await.unwrap().unwrap())
            {
                break session.id;
            }
        }
    })
    .await
    .unwrap();
    ws.send(control(&ClientMsg::Attach {
        id,
        channel: 1,
        last_seq: None,
        cols: 80,
        rows: 24,
        colors: Some(TermColors { fg: "#d4d4d4".into(), bg: "#191a1b".into() }),
    }))
    .await
    .unwrap();
    ws.send(Message::Binary(Frame::new(1, b"go\n".to_vec()).encode().into())).await.unwrap();

    let output = tokio::time::timeout(Duration::from_secs(10), async {
        let mut output = Vec::new();
        while let Some(msg) = ws.next().await {
            if let Some((1, Err(bytes))) = parse_server(msg.unwrap()) {
                output.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&output).contains("]\r\n") {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    })
    .await;
    ws.send(control(&ClientMsg::KillSession { id })).await.unwrap();
    let output = output.expect("query probe timed out");
    assert!(output.contains("REPLY[E]11;rgb:1919/1a1a/1b1bB]"), "{output:?}");
}
