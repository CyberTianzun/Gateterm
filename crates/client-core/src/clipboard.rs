use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

pub const MAX_CLIPBOARD_WRITE_BYTES: usize = 64 * 1024 * 1024;

/// Maximum raw OSC 5522 payload bytes (header + base64) before a single sequence
/// is guaranteed to exceed [`MAX_CLIPBOARD_WRITE_BYTES`].
const MAX_RAW_OSC_BYTES: usize = (MAX_CLIPBOARD_WRITE_BYTES / 3 + 1) * 4 + 4096;
const OSC5522_PREFIX: &[u8; 7] = b"\x1b]5522;";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardEvent {
    Write { mime: String, data: Vec<u8> },
    ReadRequest { mimes: Vec<String> },
}

pub fn encode_osc5522_read_reply(mime: &str, data: &[u8]) -> Vec<u8> {
    let b64_mime = STANDARD.encode(mime.as_bytes());
    let b64_data = STANDARD.encode(data);
    format!("\x1b]5522;type=read:status=DATA:mime={b64_mime};{b64_data}\x1b\\").into_bytes()
}

pub fn encode_osc5522_paste_offer(available_mimes: &[&str]) -> Vec<u8> {
    let joined = available_mimes.join(" ");
    let b64_dot = STANDARD.encode(b".");
    let b64_mimes = STANDARD.encode(joined.as_bytes());
    format!("\x1b]5522;type=read:status=DATA:mime={b64_dot};{b64_mimes}\x1b\\").into_bytes()
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum OscState {
    #[default]
    Ground,
    /// Matched `1..7` bytes of `b"\x1b]5522;"`.
    Prefix(usize),
    /// Inside `\x1b]5522;...`, accumulating payload until BEL (`0x07`) or ST (`\x1b\\`).
    Payload,
    /// Saw `0x1b` inside `Payload`; expecting `b'\\'` for ST.
    PayloadEsc,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum DecsetState {
    #[default]
    Idle,
    Esc,
    CsiStart,
    DecPrivate {
        cur_param: u32,
        has_digits: bool,
        has_5522: bool,
    },
}

#[derive(Default)]
pub struct Osc5522Parser {
    state: OscState,
    osc_buf: Vec<u8>,
    osc_overflowed: bool,
    in_multi_write: bool,
    multi_write_mime: Option<String>,
    multi_write_bufs: Vec<(String, Vec<u8>)>,
    multi_write_total: usize,
    write_overflowed: bool,
    decset_state: DecsetState,
    paste_mode: bool,
}

impl Osc5522Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn paste_mode(&self) -> bool {
        self.paste_mode
    }

    pub fn filter<F: FnMut(&[u8])>(
        &mut self,
        input: &[u8],
        mut on_event: impl FnMut(ClipboardEvent),
        mut emit: F,
    ) {
        let mut passthrough_start = 0usize;
        for (i, &b) in input.iter().enumerate() {
            match self.state {
                OscState::Ground => {
                    if b == 0x1b {
                        if i > passthrough_start {
                            let slice = &input[passthrough_start..i];
                            self.track_decset_bytes(slice);
                            emit(slice);
                        }
                        self.state = OscState::Prefix(1);
                        passthrough_start = i + 1;
                    }
                }
                OscState::Prefix(matched) => {
                    passthrough_start = i + 1;
                    if b == OSC5522_PREFIX[matched] {
                        if matched + 1 == OSC5522_PREFIX.len() {
                            self.state = OscState::Payload;
                            self.osc_buf.clear();
                            self.osc_overflowed = false;
                        } else {
                            self.state = OscState::Prefix(matched + 1);
                        }
                    } else if b == 0x1b {
                        let prefix = &OSC5522_PREFIX[..matched];
                        self.track_decset_bytes(prefix);
                        emit(prefix);
                        self.state = OscState::Prefix(1);
                    } else {
                        let prefix = &OSC5522_PREFIX[..matched];
                        self.track_decset_bytes(prefix);
                        emit(prefix);
                        let single = [b];
                        self.track_decset_bytes(&single);
                        emit(&single);
                        self.state = OscState::Ground;
                    }
                }
                OscState::Payload => {
                    passthrough_start = i + 1;
                    if b == 0x07 {
                        self.finish_osc_sequence(&mut on_event);
                        self.state = OscState::Ground;
                    } else if b == 0x1b {
                        self.state = OscState::PayloadEsc;
                    } else if !self.osc_overflowed {
                        if self.osc_buf.len() < MAX_RAW_OSC_BYTES {
                            self.osc_buf.push(b);
                        } else {
                            self.osc_buf.clear();
                            self.osc_buf.shrink_to_fit();
                            self.osc_overflowed = true;
                            if self.in_multi_write {
                                self.multi_write_bufs.clear();
                                self.multi_write_total = 0;
                                self.write_overflowed = true;
                            }
                        }
                    }
                }
                OscState::PayloadEsc => {
                    passthrough_start = i + 1;
                    if b == b'\\' {
                        self.finish_osc_sequence(&mut on_event);
                        self.state = OscState::Ground;
                    } else {
                        // Unterminated OSC 5522 aborted by a new ESC sequence.
                        self.osc_buf.clear();
                        self.osc_overflowed = false;
                        if b == b']' {
                            self.state = OscState::Prefix(2);
                        } else if b == 0x1b {
                            let esc = [0x1b];
                            self.track_decset_bytes(&esc);
                            emit(&esc);
                            self.state = OscState::Prefix(1);
                        } else {
                            let pair = [0x1b, b];
                            self.track_decset_bytes(&pair);
                            emit(&pair);
                            self.state = OscState::Ground;
                        }
                    }
                }
            }
        }
        if self.state == OscState::Ground && passthrough_start < input.len() {
            let tail = &input[passthrough_start..];
            self.track_decset_bytes(tail);
            emit(tail);
        }
    }

    fn track_decset_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.decset_state {
                DecsetState::Idle => {
                    if b == 0x1b {
                        self.decset_state = DecsetState::Esc;
                    }
                }
                DecsetState::Esc => match b {
                    b'[' => self.decset_state = DecsetState::CsiStart,
                    b'c' => {
                        self.paste_mode = false;
                        self.decset_state = DecsetState::Idle;
                    }
                    0x1b => self.decset_state = DecsetState::Esc,
                    _ => self.decset_state = DecsetState::Idle,
                },
                DecsetState::CsiStart => match b {
                    b'?' => {
                        self.decset_state = DecsetState::DecPrivate {
                            cur_param: 0,
                            has_digits: false,
                            has_5522: false,
                        };
                    }
                    0x1b => self.decset_state = DecsetState::Esc,
                    _ => self.decset_state = DecsetState::Idle,
                },
                DecsetState::DecPrivate {
                    mut cur_param,
                    mut has_digits,
                    mut has_5522,
                } => match b {
                    b'0'..=b'9' => {
                        cur_param = cur_param.saturating_mul(10).saturating_add((b - b'0') as u32);
                        has_digits = true;
                        self.decset_state = DecsetState::DecPrivate {
                            cur_param,
                            has_digits,
                            has_5522,
                        };
                    }
                    b';' => {
                        if has_digits && cur_param == 5522 {
                            has_5522 = true;
                        }
                        self.decset_state = DecsetState::DecPrivate {
                            cur_param: 0,
                            has_digits: false,
                            has_5522,
                        };
                    }
                    b'h' => {
                        if (has_digits && cur_param == 5522) || has_5522 {
                            self.paste_mode = true;
                        }
                        self.decset_state = DecsetState::Idle;
                    }
                    b'l' => {
                        if (has_digits && cur_param == 5522) || has_5522 {
                            self.paste_mode = false;
                        }
                        self.decset_state = DecsetState::Idle;
                    }
                    0x1b => self.decset_state = DecsetState::Esc,
                    _ => self.decset_state = DecsetState::Idle,
                },
            }
        }
    }

    fn finish_osc_sequence(&mut self, on_event: &mut impl FnMut(ClipboardEvent)) {
        if self.osc_overflowed {
            self.osc_buf.clear();
            self.osc_overflowed = false;
            return;
        }
        let raw = std::mem::take(&mut self.osc_buf);
        let Ok(payload) = std::str::from_utf8(&raw) else {
            return;
        };
        let (control_part, body_part) = match payload.split_once(';') {
            Some((c, b)) => (c, Some(b)),
            None => (payload, None),
        };

        let mut op_type: Option<&str> = None;
        let mut mime_raw: Option<&str> = None;
        let mut data_raw: Option<&str> = None;

        for kv in control_part.split(':') {
            if let Some((k, v)) = kv.split_once('=') {
                match k.trim() {
                    "type" => op_type = Some(v.trim()),
                    "mime" => mime_raw = Some(v.trim()),
                    "data" => data_raw = Some(v.trim()),
                    _ => {}
                }
            }
        }

        if let Some(body) = body_part {
            if let Some(rest) = body.strip_prefix("data=") {
                data_raw = Some(rest.trim());
            } else if let Some(rest) = body.strip_prefix("mime=") {
                if let Some((m, d)) = rest.split_once(';') {
                    mime_raw = Some(m.trim());
                    data_raw = Some(d.strip_prefix("data=").unwrap_or(d).trim());
                } else {
                    mime_raw = Some(rest.trim());
                }
            } else if matches!(op_type, Some("read")) {
                if mime_raw.is_none() {
                    mime_raw = Some(body.trim());
                }
            } else {
                data_raw = Some(body.trim());
            }
        }

        match op_type {
            Some("write") => {
                let decoded_mime = mime_raw.and_then(decode_mime_str);
                if let Some(d_str) = data_raw {
                    self.in_multi_write = false;
                    self.multi_write_bufs.clear();
                    self.multi_write_total = 0;
                    self.write_overflowed = false;
                    let mime = decoded_mime.unwrap_or_else(|| "text/plain".to_string());
                    if let Some(data) = decode_b64_capped(d_str) {
                        on_event(ClipboardEvent::Write { mime, data });
                    }
                } else {
                    // Start of a multi-packet write session (`type=write` followed by `type=wdata`).
                    self.in_multi_write = true;
                    self.multi_write_mime = decoded_mime;
                    self.multi_write_bufs.clear();
                    self.multi_write_total = 0;
                    self.write_overflowed = false;
                }
            }
            Some("wdata") => {
                let decoded_mime = mime_raw
                    .and_then(decode_mime_str)
                    .or_else(|| self.multi_write_mime.clone());
                match data_raw {
                    Some(d_str) if !d_str.is_empty() => {
                        let mime = decoded_mime.unwrap_or_else(|| "text/plain".to_string());
                        if self.in_multi_write {
                            if self.write_overflowed {
                                return;
                            }
                            if let Some(chunk) = decode_b64_capped(d_str) {
                                if self.multi_write_total.saturating_add(chunk.len())
                                    > MAX_CLIPBOARD_WRITE_BYTES
                                {
                                    self.multi_write_bufs.clear();
                                    self.multi_write_total = 0;
                                    self.write_overflowed = true;
                                } else {
                                    self.multi_write_total += chunk.len();
                                    if let Some((_, existing)) = self
                                        .multi_write_bufs
                                        .iter_mut()
                                        .find(|(m, _)| *m == mime)
                                    {
                                        existing.extend_from_slice(&chunk);
                                    } else {
                                        self.multi_write_bufs.push((mime, chunk));
                                    }
                                }
                            } else {
                                self.multi_write_bufs.clear();
                                self.multi_write_total = 0;
                                self.write_overflowed = true;
                            }
                        } else if let Some(data) = decode_b64_capped(d_str) {
                            on_event(ClipboardEvent::Write { mime, data });
                        }
                    }
                    _ => {
                        // Empty `type=wdata` finishes a multi-packet write session.
                        if self.in_multi_write {
                            self.in_multi_write = false;
                            let overflowed = self.write_overflowed;
                            self.write_overflowed = false;
                            self.multi_write_total = 0;
                            self.multi_write_mime = None;
                            let bufs = std::mem::take(&mut self.multi_write_bufs);
                            if !overflowed {
                                for (mime, data) in bufs {
                                    on_event(ClipboardEvent::Write { mime, data });
                                }
                            }
                        }
                    }
                }
            }
            Some("read") => {
                let mut mimes = Vec::new();
                if let Some(raw_mimes) = mime_raw {
                    for token in raw_mimes.split([' ', ':', ';', ',']) {
                        let t = token.trim();
                        if t.is_empty() {
                            continue;
                        }
                        if let Some(decoded) = decode_mime_str(t) {
                            for part in decoded.split_whitespace() {
                                if !part.is_empty() {
                                    mimes.push(part.to_string());
                                }
                            }
                        }
                    }
                }
                if !mimes.is_empty() {
                    on_event(ClipboardEvent::ReadRequest { mimes });
                }
            }
            _ => {}
        }
    }
}

fn decode_mime_str(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(bytes) = STANDARD
        .decode(trimmed)
        .or_else(|_| STANDARD_NO_PAD.decode(trimmed))
        && let Ok(utf8) = String::from_utf8(bytes)
    {
        let clean = utf8.trim();
        if !clean.is_empty() {
            return Some(clean.to_string());
        }
    }
    if trimmed == "." || trimmed.contains('/') {
        return Some(trimmed.to_string());
    }
    None
}

fn decode_b64_capped(s: &str) -> Option<Vec<u8>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    let clean: Vec<u8> = trimmed
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let pad = clean.iter().rev().take(2).filter(|&&b| b == b'=').count();
    let full_groups = (clean.len() / 4) * 3;
    let rem = (clean.len() % 4).saturating_sub(1);
    let est_decoded = full_groups.saturating_add(rem).saturating_sub(pad);
    if est_decoded > MAX_CLIPBOARD_WRITE_BYTES {
        return None;
    }
    let decoded = STANDARD
        .decode(&clean)
        .or_else(|_| STANDARD_NO_PAD.decode(&clean))
        .ok()?;
    if decoded.len() > MAX_CLIPBOARD_WRITE_BYTES {
        return None;
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use crate::clipboard::{
        ClipboardEvent, MAX_CLIPBOARD_WRITE_BYTES, encode_osc5522_paste_offer,
        encode_osc5522_read_reply,
    };
    use crate::term::ClientTerm;

    #[test]
    fn captures_osc52_and_chunked_osc5522_writes_and_read_requests() {
        let mut t = ClientTerm::new(20, 5, 100);
        // 1. OSC 52 write ("hello" -> aGVsbG8=)
        t.write(b"\x1b]52;c;aGVsbG8=\x07");
        let evs = t.drain_clipboard_events();
        assert_eq!(
            evs,
            vec![ClipboardEvent::Write {
                mime: "text/plain".into(),
                data: b"hello".to_vec()
            }]
        );

        // 2. 跨 chunk 的 OSC 5522 多格式写入 (image/png)与读取请求
        t.write(b"\x1b]5522;type=write:mime=aW1hZ2UvcG5n;");
        t.write(b"data=iVBORw==\x1b\\");
        let evs2 = t.drain_clipboard_events();
        assert_eq!(
            evs2,
            vec![ClipboardEvent::Write {
                mime: "image/png".into(),
                data: vec![0x89, 0x50, 0x4e, 0x47]
            }]
        );

        // 3. DECSET 5522 模式开关
        t.write(b"\x1b[?5522h");
        assert!(t.state().kitty_clipboard_paste_mode);
        t.write(b"\x1b[?5522l");
        assert!(!t.state().kitty_clipboard_paste_mode);

        // 4. Read request
        t.write(b"\x1b]5522;type=read;aW1hZ2UvcG5n\x1b\\");
        assert_eq!(
            t.drain_clipboard_events(),
            vec![ClipboardEvent::ReadRequest {
                mimes: vec!["image/png".into()]
            }]
        );

        let offer = encode_osc5522_paste_offer(&["text/plain", "image/png"]);
        assert!(offer.starts_with(b"\x1b]5522;"));
        let reply = encode_osc5522_read_reply("text/plain", b"ok");
        assert!(reply.starts_with(b"\x1b]5522;"));
    }

    #[test]
    fn discards_osc5522_write_exceeding_max_clipboard_write_bytes() {
        let mut t = ClientTerm::new(20, 5, 100);
        // 3 MiB decoded chunk -> 4 MiB base64 chunk with zero '=' padding so chunks concatenate cleanly
        let chunk_raw_len = 3 * 1024 * 1024;
        let b64_chunk = vec![b'A'; (chunk_raw_len / 3) * 4];

        t.write(b"\x1b]5522;type=write:mime=dGV4dC9wbGFpbg==;data=");
        let mut sent_decoded = 0usize;
        while sent_decoded <= MAX_CLIPBOARD_WRITE_BYTES {
            t.write(&b64_chunk);
            sent_decoded += chunk_raw_len;
        }
        t.write(b"\x1b\\");
        assert!(
            t.drain_clipboard_events().is_empty(),
            "write exceeding MAX_CLIPBOARD_WRITE_BYTES must be discarded"
        );

        // Multi-packet wdata session exceeding MAX_CLIPBOARD_WRITE_BYTES is also discarded
        let mut wdata_pkt = b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;data=".to_vec();
        wdata_pkt.extend_from_slice(&b64_chunk);
        wdata_pkt.extend_from_slice(b"\x1b\\");

        t.write(b"\x1b]5522;type=write\x1b\\");
        let mut multi_sent = 0usize;
        while multi_sent <= MAX_CLIPBOARD_WRITE_BYTES {
            t.write(&wdata_pkt);
            multi_sent += chunk_raw_len;
        }
        t.write(b"\x1b]5522;type=wdata\x1b\\");
        assert!(
            t.drain_clipboard_events().is_empty(),
            "multi-packet wdata exceeding MAX_CLIPBOARD_WRITE_BYTES must be discarded"
        );

        // Subsequent small write still succeeds after buffer reset
        t.write(b"\x1b]5522;type=write:mime=dGV4dC9wbGFpbg==;data=aGVsbG8=\x07");
        assert_eq!(
            t.drain_clipboard_events(),
            vec![ClipboardEvent::Write {
                mime: "text/plain".into(),
                data: b"hello".to_vec(),
            }]
        );
    }
}
