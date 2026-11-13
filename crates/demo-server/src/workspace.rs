use crate::{
    screen::Screen,
    virtual_shell::{Shell, VirtualFs},
};
use base64::Engine;
use gateterm_protocol::{AttachMode, ClientMsg, Frame, ServerMsg, SessionEventKind, SessionInfo};
use std::collections::{BTreeMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};

#[derive(Default)]
pub struct Workspace {
    fs: VirtualFs,
    sessions: BTreeMap<u64, Session>,
    clients: BTreeMap<u64, Client>,
    next_session: u64,
    next_client: u64,
}
struct Session {
    info: SessionInfo,
    shell: Shell,
    screen: Screen,
    ring: VecDeque<u8>,
    seq: u64,
    zero_width_left: usize,
    last_resize_seq: Option<u64>,
}
struct Client {
    tx: mpsc::Sender<Frame>,
    close: watch::Sender<bool>,
    attached: BTreeMap<u32, Attachment>,
}
struct Attachment {
    session: u64,
    cols: u16,
    rows: u16,
}

pub fn control_frame(msg: ServerMsg) -> Frame {
    Frame::new(0, serde_json::to_vec(&msg).expect("protocol serialization"))
}
fn geometry(cols: u16, rows: u16) -> (u16, u16) {
    // A dense Unicode screen plus base64/JSON preview must fit 64KiB too.
    (cols.clamp(20, 160), rows.clamp(5, 60))
}

impl Workspace {
    pub fn connect(&mut self, tx: mpsc::Sender<Frame>, close: watch::Sender<bool>) -> u64 {
        self.next_client += 1;
        let id = self.next_client;
        self.clients.insert(
            id,
            Client {
                tx,
                close,
                attached: BTreeMap::new(),
            },
        );
        id
    }
    fn send(&self, conn: u64, frame: Frame) {
        if let Some(client) = self.clients.get(&conn) {
            if *client.close.borrow() {
                return;
            }
            if client.tx.try_send(frame).is_err() {
                client.close.send_replace(true);
            }
        }
    }
    fn message(&self, conn: u64, msg: ServerMsg) {
        self.send(conn, control_frame(msg));
    }
    pub fn error(&self, conn: u64, channel: Option<u32>, message: &str) {
        self.message(
            conn,
            ServerMsg::Error {
                message: message.into(),
                channel,
            },
        );
    }
    fn event(&self, event: SessionEventKind, session: SessionInfo) {
        let frame = control_frame(ServerMsg::SessionEvent { event, session });
        for &conn in self.clients.keys() {
            self.send(conn, frame.clone());
        }
    }
    fn attachments(&self, session: u64) -> Vec<(u64, u32)> {
        self.clients
            .iter()
            .flat_map(|(&conn, c)| {
                c.attached
                    .iter()
                    .filter_map(move |(&ch, a)| (a.session == session).then_some((conn, ch)))
            })
            .collect()
    }
    fn resized(&self, conn: u64, channel: u32, session: u64) {
        let info = &self.sessions[&session].info;
        self.message(
            conn,
            ServerMsg::Resized {
                channel,
                cols: info.cols,
                rows: info.rows,
            },
        );
    }
    fn resync(&self, conn: u64, channel: u32, session: u64) {
        self.resized(conn, channel, session);
        self.message(conn, ServerMsg::ResyncBegin { channel });
        self.send(
            conn,
            Frame::new(channel, self.sessions[&session].screen.repaint()),
        );
        self.message(
            conn,
            ServerMsg::ResyncEnd {
                channel,
                seq: self.sessions[&session].seq,
            },
        );
    }
    fn merge(&mut self, session: u64, except: Option<(u64, u32)>) {
        let sizes: Vec<_> = self
            .clients
            .values()
            .flat_map(|c| c.attached.values())
            .filter(|a| a.session == session)
            .map(|a| (a.cols, a.rows))
            .collect();
        if sizes.is_empty() {
            return;
        }
        let cols = sizes.iter().map(|s| s.0).min().unwrap();
        let rows = sizes.iter().map(|s| s.1).min().unwrap();
        let Some(s) = self.sessions.get_mut(&session) else {
            return;
        };
        if (s.info.cols, s.info.rows) == (cols, rows) {
            return;
        }
        s.info.cols = cols;
        s.info.rows = rows;
        s.last_resize_seq = Some(s.seq);
        s.screen.resize(cols, rows);
        for (conn, ch) in self.attachments(session) {
            if except != Some((conn, ch)) {
                self.resync(conn, ch, session);
            }
        }
    }
    fn output(&mut self, session: u64, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let s = self.sessions.get_mut(&session).unwrap();
        // Alacritty stores combining marks in per-cell vectors. Bound them
        // before both the live stream and grid so the two representations agree.
        // 9600 * 4 base bytes + duplicated 1024 * 4 combining bytes + CUP,
        // including base64/JSON expansion, is < 64KiB.
        let bytes = std::str::from_utf8(&bytes)
            .expect("virtual output is UTF-8")
            .chars()
            .map(|c| {
                if unicode_width::UnicodeWidthChar::width(c) == Some(0) {
                    if s.zero_width_left == 0 {
                        return '?';
                    }
                    s.zero_width_left -= 1;
                }
                c
            })
            .collect::<String>()
            .into_bytes();
        s.screen.advance(&bytes);
        s.seq += bytes.len() as u64;
        s.ring.extend(&bytes);
        if s.ring.len() > 65536 {
            s.ring.drain(..s.ring.len() - 65536);
        }
        for (conn, ch) in self.attachments(session) {
            self.send(conn, Frame::new(ch, bytes.clone()));
        }
    }
    pub fn control(&mut self, conn: u64, msg: ClientMsg) {
        if !self.clients.contains_key(&conn) {
            return;
        }
        match msg {
            ClientMsg::Hello { .. } => self.error(conn, None, "already authenticated"),
            ClientMsg::ListSessions => self.message(
                conn,
                ServerMsg::Sessions {
                    sessions: self.sessions.values().map(|s| s.info.clone()).collect(),
                },
            ),
            ClientMsg::NewSession { name, command, cwd } => {
                if command
                    .as_ref()
                    .is_some_and(|argv| argv.as_slice() != ["demo"])
                {
                    self.error(conn, None, "only the virtual demo launcher is allowed");
                    return;
                }
                if self.sessions.len() >= 8 {
                    self.error(conn, None, "virtual session quota exceeded");
                    return;
                }
                if name.as_ref().is_some_and(|name| !valid_name(name)) {
                    self.error(conn, None, "invalid virtual session name");
                    return;
                }
                let cwd = cwd
                    .and_then(|p| self.fs.directory(&p))
                    .unwrap_or_else(|| "/workspace".into());
                self.next_session += 1;
                let id = self.next_session;
                let info = SessionInfo {
                    id,
                    name: format!(
                        "Virtual demo — {}",
                        name.unwrap_or_else(|| format!("terminal {id}"))
                    ),
                    cols: 80,
                    rows: 24,
                    created_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    cwd: Some(cwd.clone()),
                };
                let shell = Shell::new(&cwd);
                let banner = format!(
                    "Virtual demo — no host shell / no real files\r\nIn-memory workspace. Type help to see virtual commands.\r\n{}",
                    shell.prompt()
                );
                self.sessions.insert(
                    id,
                    Session {
                        info: info.clone(),
                        shell,
                        screen: Screen::new(80, 24),
                        ring: VecDeque::with_capacity(65536),
                        seq: 0,
                        zero_width_left: 1024,
                        last_resize_seq: None,
                    },
                );
                self.output(id, banner.into_bytes());
                self.event(SessionEventKind::Created, info);
            }
            ClientMsg::RenameSession { id, name } => {
                if !valid_name(&name) {
                    self.error(conn, None, "invalid virtual session name");
                    return;
                }
                let Some(s) = self.sessions.get_mut(&id) else {
                    self.error(conn, None, "no such session");
                    return;
                };
                s.info.name = format!("Virtual demo — {name}");
                let info = s.info.clone();
                self.event(SessionEventKind::Renamed, info);
            }
            ClientMsg::KillSession { id } => {
                let Some(s) = self.sessions.remove(&id) else {
                    self.error(conn, None, "no such session");
                    return;
                };
                for (c, ch) in self.attachments(id) {
                    self.message(c, ServerMsg::SessionExited { channel: ch, id });
                    self.clients.get_mut(&c).unwrap().attached.remove(&ch);
                }
                self.event(SessionEventKind::Closed, s.info);
            }
            ClientMsg::Preview { id } => {
                if let Some(s) = self.sessions.get(&id) {
                    self.message(
                        conn,
                        ServerMsg::PreviewData {
                            id,
                            data: base64::engine::general_purpose::STANDARD
                                .encode(s.screen.repaint()),
                        },
                    );
                } else {
                    self.error(conn, None, "no such session");
                }
            }
            ClientMsg::Attach {
                id,
                channel,
                last_seq,
                cols,
                rows,
                ..
            } => {
                let client = &self.clients[&conn];
                if channel == 0
                    || client.attached.contains_key(&channel)
                    || client.attached.len() >= 8
                {
                    self.error(conn, Some(channel), "channel unavailable");
                    return;
                }
                if !self.sessions.contains_key(&id) {
                    self.error(conn, Some(channel), "no such session");
                    return;
                }
                let (cols, rows) = geometry(cols, rows);
                self.clients.get_mut(&conn).unwrap().attached.insert(
                    channel,
                    Attachment {
                        session: id,
                        cols,
                        rows,
                    },
                );
                self.merge(id, Some((conn, channel)));
                let s = &self.sessions[&id];
                let start = s.seq - s.ring.len() as u64;
                if let Some(n) = last_seq.filter(|&n| {
                    n >= start
                        && n <= s.seq
                        && s.last_resize_seq.is_none_or(|resize_seq| n > resize_seq)
                }) {
                    let seq = s.seq;
                    let bytes = s
                        .ring
                        .iter()
                        .skip((n - start) as usize)
                        .copied()
                        .collect::<Vec<_>>();
                    self.message(
                        conn,
                        ServerMsg::Attached {
                            channel,
                            seq,
                            mode: AttachMode::Replay,
                        },
                    );
                    self.resized(conn, channel, id);
                    if !bytes.is_empty() {
                        self.send(conn, Frame::new(channel, bytes));
                    }
                } else {
                    self.message(
                        conn,
                        ServerMsg::Attached {
                            channel,
                            seq: 0,
                            mode: AttachMode::Resync,
                        },
                    );
                    self.resync(conn, channel, id);
                }
            }
            ClientMsg::Detach { channel } => {
                if let Some(a) = self
                    .clients
                    .get_mut(&conn)
                    .unwrap()
                    .attached
                    .remove(&channel)
                {
                    self.merge(a.session, None);
                } else {
                    self.error(conn, Some(channel), "no such channel");
                }
            }
            ClientMsg::Resize {
                channel,
                cols,
                rows,
                ..
            } => {
                if let Some(a) = self
                    .clients
                    .get_mut(&conn)
                    .unwrap()
                    .attached
                    .get_mut(&channel)
                {
                    (a.cols, a.rows) = geometry(cols, rows);
                    let session = a.session;
                    self.merge(session, None);
                } else {
                    self.error(conn, Some(channel), "no such channel");
                }
            }
        }
    }
    pub fn input(&mut self, conn: u64, channel: u32, bytes: &[u8]) {
        let session = self
            .clients
            .get(&conn)
            .and_then(|c| c.attached.get(&channel))
            .map(|a| a.session);
        if let Some(id) = session {
            let s = self.sessions.get_mut(&id).unwrap();
            let bytes = s.shell.input(&mut self.fs, bytes);
            s.info.cwd = Some(s.shell.cwd.clone());
            self.output(id, bytes);
        } else {
            self.error(conn, Some(channel), "no such channel");
        }
    }
    pub fn disconnect(&mut self, conn: u64) {
        if let Some(c) = self.clients.remove(&conn) {
            for a in c.attached.values() {
                self.merge(a.session, None);
            }
        }
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 128 && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateterm_protocol::{AttachMode, ServerMsg};
    fn client(ws: &mut Workspace) -> (u64, mpsc::Receiver<Frame>) {
        let (tx, rx) = mpsc::channel(64);
        let (close, _) = watch::channel(false);
        (ws.connect(tx, close), rx)
    }
    fn drain(rx: &mut mpsc::Receiver<Frame>) -> Vec<Frame> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }
    fn controls(frames: &[Frame]) -> Vec<ServerMsg> {
        frames
            .iter()
            .filter(|f| f.channel == 0)
            .map(|f| serde_json::from_slice(&f.payload).unwrap())
            .collect()
    }
    fn create(ws: &mut Workspace, id: u64) {
        ws.control(
            id,
            ClientMsg::NewSession {
                name: None,
                command: None,
                cwd: None,
            },
        );
    }
    fn attach(ws: &mut Workspace, conn: u64, channel: u32, seq: Option<u64>, cols: u16, rows: u16) {
        ws.control(
            conn,
            ClientMsg::Attach {
                id: 1,
                channel,
                last_seq: seq,
                cols,
                rows,
                colors: None,
            },
        );
    }

    #[test]
    fn attach_replay_resync_and_reconnect_retain_sequence() {
        let mut ws = Workspace::default();
        let (id, mut rx) = client(&mut ws);
        create(&mut ws, id);
        drain(&mut rx);
        attach(&mut ws, id, 7, None, 80, 24);
        let frames = drain(&mut rx);
        let msgs = controls(&frames);
        assert!(matches!(
            msgs.as_slice(),
            [
                ServerMsg::Attached {
                    mode: AttachMode::Resync,
                    seq: 0,
                    ..
                },
                ServerMsg::Resized { .. },
                ServerMsg::ResyncBegin { .. },
                ServerMsg::ResyncEnd { .. }
            ]
        ));
        assert_eq!(frames[3].channel, 7);
        let seq = match msgs.last().unwrap() {
            ServerMsg::ResyncEnd { seq, .. } => *seq,
            _ => panic!(),
        };
        ws.input(id, 7, b"echo retained\r");
        let output = drain(&mut rx);
        assert!(
            output
                .iter()
                .any(|f| String::from_utf8_lossy(&f.payload).contains("retained\r\n"))
        );
        ws.disconnect(id);
        let (id, mut rx) = client(&mut ws);
        attach(&mut ws, id, 8, Some(seq), 80, 24);
        let frames = drain(&mut rx);
        assert!(matches!(
            controls(&frames).as_slice(),
            [
                ServerMsg::Attached {
                    mode: AttachMode::Replay,
                    ..
                },
                ServerMsg::Resized { .. }
            ]
        ));
        assert!(String::from_utf8_lossy(&frames[2].payload).contains("retained\r\n"));
        ws.control(id, ClientMsg::Detach { channel: 8 });
        attach(&mut ws, id, 8, Some(u64::MAX), 80, 24);
        assert!(
            controls(&drain(&mut rx))
                .iter()
                .any(|m| matches!(m, ServerMsg::ResyncEnd { .. }))
        );
    }

    #[test]
    fn geometry_and_output_broadcast_to_shared_attachments() {
        let mut ws = Workspace::default();
        let (a, mut ra) = client(&mut ws);
        let (b, mut rb) = client(&mut ws);
        create(&mut ws, a);
        assert_eq!(controls(&drain(&mut ra)), controls(&drain(&mut rb)));
        attach(&mut ws, a, 1, None, 80, 24);
        drain(&mut ra);
        attach(&mut ws, b, 9, None, 40, 10);
        assert!(matches!(
            controls(&drain(&mut ra))[0],
            ServerMsg::Resized {
                channel: 1,
                cols: 40,
                rows: 10
            }
        ));
        let frames = drain(&mut rb);
        assert!(matches!(
            controls(&frames)[1],
            ServerMsg::Resized {
                channel: 9,
                cols: 40,
                rows: 10
            }
        ));
        ws.input(b, 9, b"echo shared\r");
        let fa = drain(&mut ra);
        let fb = drain(&mut rb);
        assert_eq!(fa[0].payload, fb[0].payload);
        assert_eq!(fa[0].channel, 1);
        assert_eq!(fb[0].channel, 9);
        ws.disconnect(b);
        assert!(matches!(
            controls(&drain(&mut ra))[0],
            ServerMsg::Resized {
                cols: 80,
                rows: 24,
                ..
            }
        ));
        ws.control(a, ClientMsg::KillSession { id: 1 });
        let msgs = controls(&drain(&mut ra));
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::SessionExited { channel: 1, id: 1 }))
        );
    }

    #[test]
    fn unapproved_commands_and_session_or_attachment_overflow_are_rejected() {
        let mut ws = Workspace::default();
        let (id, mut rx) = client(&mut ws);
        for command in [
            vec![],
            vec!["powershell".into()],
            vec!["demo".into(), "bad".into()],
        ] {
            ws.control(
                id,
                ClientMsg::NewSession {
                    name: None,
                    command: Some(command),
                    cwd: None,
                },
            );
            assert!(matches!(
                controls(&drain(&mut rx)).as_slice(),
                [ServerMsg::Error { channel: None, .. }]
            ));
        }
        for _ in 0..8 {
            create(&mut ws, id);
            drain(&mut rx);
        }
        create(&mut ws, id);
        assert!(matches!(
            controls(&drain(&mut rx))[0],
            ServerMsg::Error { .. }
        ));
        for channel in 1..=8 {
            attach(&mut ws, id, channel, None, 0, u16::MAX);
            drain(&mut rx);
        }
        attach(&mut ws, id, 9, None, 80, 24);
        assert!(matches!(
            controls(&drain(&mut rx))[0],
            ServerMsg::Error {
                channel: Some(9),
                ..
            }
        ));
    }

    #[test]
    fn slow_queue_overflow_closes_only_consumer_and_keeps_state() {
        let mut ws = Workspace::default();
        let (tx, mut rx) = mpsc::channel(1);
        let (close, closed) = watch::channel(false);
        let slow = ws.connect(tx, close);
        create(&mut ws, slow);
        create(&mut ws, slow);
        assert!(*closed.borrow());
        rx.try_recv().unwrap();
        ws.disconnect(slow);
        let (id, mut rx) = client(&mut ws);
        ws.control(id, ClientMsg::ListSessions);
        assert!(
            matches!(&controls(&drain(&mut rx))[0],ServerMsg::Sessions{sessions} if sessions.len()==2)
        );
    }

    #[test]
    fn combining_mark_flood_cannot_grow_repaint_past_one_bounded_frame() {
        let mut ws = Workspace::default();
        let (id, mut rx) = client(&mut ws);
        create(&mut ws, id);
        drain(&mut rx);
        attach(&mut ws, id, 1, None, 160, 80);
        drain(&mut rx);
        let line = format!("echo a{}\r", "\u{301}".repeat(1000));
        for _ in 0..80 {
            ws.input(id, 1, line.as_bytes());
            drain(&mut rx);
        }
        ws.control(id, ClientMsg::Detach { channel: 1 });
        attach(&mut ws, id, 1, None, 160, 80);
        let frames = drain(&mut rx);
        assert!(
            frames.iter().all(|f| f.payload.len() <= 65536),
            "repaint must fit exactly one bounded frame"
        );
    }

    #[test]
    fn evicted_ring_anchor_resyncs_and_current_anchor_replays_no_bytes() {
        let mut ws = Workspace::default();
        let (id, mut rx) = client(&mut ws);
        create(&mut ws, id);
        drain(&mut rx);
        attach(&mut ws, id, 1, None, 80, 24);
        drain(&mut rx);
        let line = format!("echo {}\r", "x".repeat(4000));
        for _ in 0..12 {
            ws.input(id, 1, line.as_bytes());
            drain(&mut rx);
        }
        ws.control(id, ClientMsg::Detach { channel: 1 });
        attach(&mut ws, id, 1, Some(1), 80, 24);
        let frames = drain(&mut rx);
        let msgs = controls(&frames);
        assert!(matches!(
            msgs[0],
            ServerMsg::Attached {
                mode: AttachMode::Resync,
                ..
            }
        ));
        let seq = match msgs.last().unwrap() {
            ServerMsg::ResyncEnd { seq, .. } => *seq,
            _ => panic!(),
        };
        ws.control(id, ClientMsg::Detach { channel: 1 });
        attach(&mut ws, id, 1, Some(seq), 80, 24);
        let frames = drain(&mut rx);
        assert_eq!(frames.len(), 2);
        assert!(matches!(
            controls(&frames)[0],
            ServerMsg::Attached {
                mode: AttachMode::Replay,
                ..
            }
        ));
        ws.control(id, ClientMsg::Detach { channel: 1 });
        ws.control(id, ClientMsg::ListSessions);
        assert!(
            matches!(&controls(&drain(&mut rx))[0],ServerMsg::Sessions{sessions} if sessions[0].cols==80&&sessions[0].rows==24)
        );
    }

    #[test]
    fn dense_unicode_preview_including_json_base64_stays_within_frame_limit() {
        let mut ws = Workspace::default();
        let (id, mut rx) = client(&mut ws);
        create(&mut ws, id);
        drain(&mut rx);
        attach(&mut ws, id, 1, None, 160, 80);
        drain(&mut rx);
        assert_eq!(unicode_width::UnicodeWidthChar::width('\u{1d11e}'), Some(1));
        let line = format!("{}\r\n", "\u{1d11e}".repeat(160));
        for _ in 0..80 {
            ws.output(1, line.as_bytes().to_vec());
            drain(&mut rx);
        }
        ws.output(
            1,
            format!("{}{}", "\u{1d11e}".repeat(160), "\u{301}".repeat(1024)).into_bytes(),
        );
        drain(&mut rx);
        ws.control(id, ClientMsg::Preview { id: 1 });
        let frames = drain(&mut rx);
        assert_eq!(frames.len(), 1);
        assert!(
            frames[0].payload.len() <= 65536,
            "base64 plus JSON must also fit"
        );
    }

    #[test]
    fn resize_invalidates_replay_even_when_seq_and_final_dimensions_match() {
        let mut ws = Workspace::default();
        let (id, mut rx) = client(&mut ws);
        create(&mut ws, id);
        drain(&mut rx);
        attach(&mut ws, id, 1, None, 80, 24);
        let frames = drain(&mut rx);
        let seq = match controls(&frames).last().unwrap() {
            ServerMsg::ResyncEnd { seq, .. } => *seq,
            _ => panic!(),
        };
        ws.control(id, ClientMsg::Detach { channel: 1 });
        let (other, mut other_rx) = client(&mut ws);
        attach(&mut ws, other, 1, Some(seq), 20, 5);
        assert!(
            matches!(
                controls(&drain(&mut other_rx))[0],
                ServerMsg::Attached {
                    mode: AttachMode::Resync,
                    ..
                }
            ),
            "attach resize invalidates same-seq replay"
        );
        ws.control(
            other,
            ClientMsg::Resize {
                channel: 1,
                cols: 80,
                rows: 24,
                colors: None,
            },
        );
        drain(&mut other_rx);
        ws.disconnect(other);
        attach(&mut ws, id, 1, Some(seq), 80, 24);
        assert!(
            matches!(
                controls(&drain(&mut rx))[0],
                ServerMsg::Attached {
                    mode: AttachMode::Resync,
                    ..
                }
            ),
            "same final dimensions still require fresh grid"
        );
    }
}
