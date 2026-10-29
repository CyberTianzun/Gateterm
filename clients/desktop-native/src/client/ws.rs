//! WebSocket binary protocol client for Gateterm.
//!
//! Implements single-socket multiplexing (`CONTROL_CHANNEL = 0` + terminal data channels),
//! `Hello` / `HelloOk` handshake, automatic exponential backoff reconnect (`Backoff`),
//! and sequence-number resume tracking (`ResumeTracker`).

use std::collections::HashMap;

use futures_util::{SinkExt, StreamExt};
use gateterm_client_core::{Backoff, ResumeTracker};
use gateterm_protocol::{
    AttachMode, CONTROL_CHANNEL, ClientMsg, Frame, PROTO_VERSION, ServerMsg, TermColors,
};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WsConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}

impl WsConnectionState {
    pub fn label(&self, locale: crate::i18n::Locale) -> &'static str {
        match self {
            Self::Disconnected => locale.pick("未连接", "Disconnected"),
            Self::Connecting => locale.pick("正在连接…", "Connecting..."),
            Self::Connected => locale.pick("已连接", "Connected"),
            Self::Reconnecting => locale.pick("正在重连…", "Reconnecting..."),
            Self::Error(_) => locale.pick("连接错误", "Error"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WsCommand {
    Control(ClientMsg),
    Attach {
        session_id: u64,
        channel: u32,
        cols: u16,
        rows: u16,
        colors: Option<TermColors>,
    },
    Detach {
        channel: u32,
    },
    Resize {
        channel: u32,
        cols: u16,
        rows: u16,
        colors: Option<TermColors>,
    },
    Input {
        channel: u32,
        bytes: Vec<u8>,
    },
    Disconnect,
}

#[derive(Debug, Clone)]
pub enum WsEvent {
    Connecting,
    Connected {
        os: String,
        win32_input_mode: bool,
        cloud_server_id: Option<String>,
    },
    Reconnecting,
    Disconnected,
    FatalAuthError(String),
    Control(ServerMsg),
    ChannelData {
        channel: u32,
        payload: Vec<u8>,
    },
}

struct AttachmentRecord {
    session_id: u64,
    cols: u16,
    rows: u16,
    colors: Option<TermColors>,
    tracker: ResumeTracker,
    /// The `last_seq` sent on the most recent `Attach` request for this channel.
    sent_resume_point: Option<u64>,
}

pub fn spawn_ws_worker(
    ws_url: String,
    token: String,
    mut cmd_rx: mpsc::UnboundedReceiver<WsCommand>,
    ev_tx: mpsc::UnboundedSender<WsEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Backoff::new();
        let mut attachments: HashMap<u32, AttachmentRecord> = HashMap::new();

        loop {
            let _ = ev_tx.send(WsEvent::Connecting);
            let mut auth_ok = false;
            let mut pre_hello_error: Option<String> = None;

            match connect_async(&ws_url).await {
                Ok((ws_stream, _)) => {
                    let (mut write, mut read) = ws_stream.split();
                    let hello = ClientMsg::Hello {
                        token: token.clone(),
                        proto: PROTO_VERSION,
                    };
                    let hello_frame =
                        Frame::new(CONTROL_CHANNEL, serde_json::to_vec(&hello).unwrap()).encode();

                    if write.send(Message::Binary(hello_frame.into())).await.is_err() {
                        let _ = ev_tx.send(WsEvent::Reconnecting);
                        tokio::time::sleep(backoff.next_delay()).await;
                        continue;
                    }

                    loop {
                        tokio::select! {
                            maybe_cmd = cmd_rx.recv() => {
                                let Some(cmd) = maybe_cmd else {
                                    let _ = write.close().await;
                                    let _ = ev_tx.send(WsEvent::Disconnected);
                                    return;
                                };
                                match cmd {
                                    WsCommand::Disconnect => {
                                        let _ = write.close().await;
                                        let _ = ev_tx.send(WsEvent::Disconnected);
                                        return;
                                    }
                                    WsCommand::Attach { session_id, channel, cols, rows, colors } => {
                                        let last_seq = attachments.get(&channel).and_then(|a| a.tracker.last_seq());
                                        attachments.insert(channel, AttachmentRecord {
                                            session_id,
                                            cols,
                                            rows,
                                            colors: colors.clone(),
                                            tracker: {
                                                let mut t = ResumeTracker::new();
                                                if let Some(seq) = last_seq {
                                                    t.on_attach_replay(seq);
                                                }
                                                t
                                            },
                                            sent_resume_point: last_seq,
                                        });
                                        if auth_ok {
                                            let msg = ClientMsg::Attach {
                                                id: session_id,
                                                channel,
                                                last_seq,
                                                cols,
                                                rows,
                                                colors,
                                            };
                                            let raw = Frame::new(CONTROL_CHANNEL, serde_json::to_vec(&msg).unwrap()).encode();
                                            let _ = write.send(Message::Binary(raw.into())).await;
                                        }
                                    }
                                    WsCommand::Detach { channel } => {
                                        attachments.remove(&channel);
                                        if auth_ok {
                                            let msg = ClientMsg::Detach { channel };
                                            let raw = Frame::new(CONTROL_CHANNEL, serde_json::to_vec(&msg).unwrap()).encode();
                                            let _ = write.send(Message::Binary(raw.into())).await;
                                        }
                                    }
                                    WsCommand::Resize { channel, cols, rows, colors } => {
                                        if let Some(rec) = attachments.get_mut(&channel) {
                                            rec.cols = cols;
                                            rec.rows = rows;
                                            if colors.is_some() {
                                                rec.colors = colors.clone();
                                            }
                                        }
                                        if auth_ok {
                                            let msg = ClientMsg::Resize { channel, cols, rows, colors };
                                            let raw = Frame::new(CONTROL_CHANNEL, serde_json::to_vec(&msg).unwrap()).encode();
                                            let _ = write.send(Message::Binary(raw.into())).await;
                                        }
                                    }
                                    WsCommand::Input { channel, bytes } => {
                                        if auth_ok {
                                            let raw = Frame::new(channel, bytes).encode();
                                            let _ = write.send(Message::Binary(raw.into())).await;
                                        }
                                    }
                                    WsCommand::Control(msg) => {
                                        if auth_ok {
                                            let raw = Frame::new(CONTROL_CHANNEL, serde_json::to_vec(&msg).unwrap()).encode();
                                            let _ = write.send(Message::Binary(raw.into())).await;
                                        }
                                    }
                                }
                            }
                            maybe_msg = read.next() => {
                                match maybe_msg {
                                    Some(Ok(Message::Binary(data))) => {
                                        let Ok(frame) = Frame::decode(&data) else { continue };
                                        if frame.channel != CONTROL_CHANNEL {
                                            if let Some(rec) = attachments.get_mut(&frame.channel) {
                                                rec.tracker.advance(frame.payload.len());
                                            }
                                            let _ = ev_tx.send(WsEvent::ChannelData {
                                                channel: frame.channel,
                                                payload: frame.payload,
                                            });
                                            continue;
                                        }
                                        let Ok(server_msg) = serde_json::from_slice::<ServerMsg>(&frame.payload) else {
                                            continue;
                                        };
                                        match &server_msg {
                                            ServerMsg::Error { message, .. } => {
                                                if !auth_ok {
                                                    pre_hello_error = Some(message.clone());
                                                }
                                            }
                                            ServerMsg::HelloOk { os, win32_input_mode, cloud_server_id, .. } => {
                                                auth_ok = true;
                                                backoff.reset();
                                                let _ = ev_tx.send(WsEvent::Connected {
                                                    os: os.clone(),
                                                    win32_input_mode: *win32_input_mode,
                                                    cloud_server_id: cloud_server_id.clone(),
                                                });
                                                // Request session list and re-attach all channels
                                                let list_raw = Frame::new(
                                                    CONTROL_CHANNEL,
                                                    serde_json::to_vec(&ClientMsg::ListSessions).unwrap(),
                                                ).encode();
                                                let _ = write.send(Message::Binary(list_raw.into())).await;

                                                for (&channel, rec) in attachments.iter_mut() {
                                                    let last_seq = rec.tracker.last_seq();
                                                    rec.sent_resume_point = last_seq;
                                                    let attach_msg = ClientMsg::Attach {
                                                        id: rec.session_id,
                                                        channel,
                                                        last_seq,
                                                        cols: rec.cols,
                                                        rows: rec.rows,
                                                        colors: rec.colors.clone(),
                                                    };
                                                    let raw = Frame::new(
                                                        CONTROL_CHANNEL,
                                                        serde_json::to_vec(&attach_msg).unwrap(),
                                                    ).encode();
                                                    let _ = write.send(Message::Binary(raw.into())).await;
                                                }
                                            }
                                            ServerMsg::Attached { channel, mode: AttachMode::Replay, .. } => {
                                                if let Some(rec) = attachments.get_mut(channel)
                                                    && let Some(resume_point) = rec.sent_resume_point
                                                {
                                                    rec.tracker.on_attach_replay(resume_point);
                                                }
                                            }
                                            ServerMsg::ResyncEnd { channel, seq } => {
                                                if let Some(rec) = attachments.get_mut(channel) {
                                                    rec.tracker.on_resync_end(*seq);
                                                }
                                            }
                                            _ => {}
                                        }
                                        let _ = ev_tx.send(WsEvent::Control(server_msg));
                                    }
                                    Some(Ok(Message::Ping(payload))) => {
                                        let _ = write.send(Message::Pong(payload)).await;
                                    }
                                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(_) => {}
            }

            if !auth_ok && let Some(err) = pre_hello_error {
                let _ = ev_tx.send(WsEvent::FatalAuthError(err));
                return;
            }

            let _ = ev_tx.send(WsEvent::Reconnecting);
            let delay = backoff.next_delay();
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                maybe_cmd = cmd_rx.recv() => {
                    match maybe_cmd {
                        None | Some(WsCommand::Disconnect) => {
                            let _ = ev_tx.send(WsEvent::Disconnected);
                            return;
                        }
                        Some(WsCommand::Attach { session_id, channel, cols, rows, colors }) => {
                            attachments.insert(channel, AttachmentRecord {
                                session_id,
                                cols,
                                rows,
                                colors,
                                tracker: ResumeTracker::new(),
                                sent_resume_point: None,
                            });
                        }
                        Some(WsCommand::Detach { channel }) => {
                            attachments.remove(&channel);
                        }
                        _ => {}
                    }
                }
            }
        }
    })
}
