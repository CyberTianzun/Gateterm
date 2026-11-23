//! 从 PTY 输出流里剥出 OSC 52(「复制到剪贴板」)。
//!
//! 为什么要在服务端剥:一个会话的输出字节会原样广播给**所有** attach,还会进环形
//! 缓冲供断线重连 replay。OSC 52 留在字节流里,意味着
//!
//! - 同一会话开在手机、平板、另一台电脑上时,一次拖选复制会改掉**每一端**的剪贴板;
//! - 任何一端重连拿到 replay,都会把很久以前复制过的内容再写一遍剪贴板。
//!
//! 所以这里把它从流里拿掉,解出文本交给会话,由会话定向发给「最近一次输入的那一端」
//! (见 `Session::deliver_clipboard`)。查询(`52;c;?`)直接丢弃:回答它等于让会话里
//! 的程序读远端剪贴板,不做。
//!
//! 只认 7-bit 形式 `ESC ] 52 ; <sel> ; <base64> (BEL | ESC \)`。8-bit C1(0x9D/0x9C)
//! 在 UTF-8 输出里会和多字节字符的续字节撞车,不识别。

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const PREFIX_52: &[u8] = b"52;";
const PREFIX_5522: &[u8] = b"5522;";
const MAX_KITTY_ID_LEN: usize = 64;

const ENGINE: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// 一条 OSC 52 负载(`<sel>;<base64>`)或 OSC 5522 帧的上限。超过的整条丢弃,不往外漏字节。
/// 2 MiB base64 ≈ 1.5 MiB 文本;真正投递时还有 1 MiB 文本上限
/// (`pty::MAX_CLIPBOARD_TEXT`)。各家 agent 自己的上限是 100 KB 量级。
pub const MAX_PAYLOAD: usize = 2 * 1024 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FilterResult {
    pub bytes: Vec<u8>,
    pub copies: Vec<String>,
    pub replies: Vec<Vec<u8>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    /// 看到 ESC,还不知道是不是 OSC
    Esc,
    /// `ESC ]` 之后,正在比对 `52;` 或 `5522;`
    Prefix,
    /// OSC 52/5522 负载
    Body,
    /// 负载里看到 ESC,等 `\`
    BodyEsc,
    /// 超长的 OSC 52/5522:吞到结束符为止,不产出文本事件
    Discard,
    DiscardEsc,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum OscKind {
    #[default]
    Osc52,
    Osc5522,
}

#[derive(Debug, Default)]
struct KittyWriteState {
    id: Option<String>,
    current_mime: Option<String>,
    text_b64: Vec<u8>,
    overflowed: bool,
    invalid: bool,
    replied: bool,
}

#[derive(Debug, Clone)]
enum DiscardMeta {
    Write { id: Option<String> },
    Read { id: Option<String> },
}

#[derive(Default)]
pub struct Osc52Filter {
    state: State,
    osc_kind: OscKind,
    /// Prefix 状态下是 `ESC ]` 之后已读的字节;Body 状态下是负载。
    buf: Vec<u8>,
    kitty_write: Option<KittyWriteState>,
    discard_meta: Option<DiscardMeta>,
}

impl Osc52Filter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 过滤一段输出:返回去掉 OSC 52/5522 之后的字节,以及其间解出的待复制文本。
    ///
    /// 跨批次切开的序列会被正确拼接 —— 没走完的前缀先扣在手里,下一批再决定放行
    /// 还是吞掉。
    pub fn filter(&mut self, input: &[u8]) -> (Vec<u8>, Vec<String>) {
        let res = self.filter_with_replies(input);
        (res.bytes, res.copies)
    }

    /// 过滤一段输出:返回去掉 OSC 52/5522 之后的字节、解出的待复制文本,以及需回写给 PTY 的应答。
    pub fn filter_with_replies(&mut self, input: &[u8]) -> FilterResult {
        let mut out = Vec::with_capacity(input.len());
        let mut copies = Vec::new();
        let mut replies = Vec::new();
        let mut i = 0;
        while i < input.len() {
            let b = input[i];
            match self.state {
                State::Ground => {
                    // 快路径:整段没有 ESC 时直接拷贝
                    match input[i..].iter().position(|&c| c == ESC) {
                        Some(off) => {
                            out.extend_from_slice(&input[i..i + off]);
                            self.state = State::Esc;
                            i += off;
                        }
                        None => {
                            out.extend_from_slice(&input[i..]);
                            i = input.len();
                            continue;
                        }
                    }
                }
                State::Esc => {
                    if b == b']' {
                        self.state = State::Prefix;
                        self.buf.clear();
                    } else {
                        out.push(ESC);
                        if b != ESC {
                            out.push(b);
                            self.state = State::Ground;
                        }
                    }
                }
                State::Prefix => {
                    self.buf.push(b);
                    let n = self.buf.len();
                    if self.buf.as_slice() == PREFIX_52 {
                        self.osc_kind = OscKind::Osc52;
                        self.state = State::Body;
                        self.buf.clear();
                    } else if self.buf.as_slice() == PREFIX_5522 {
                        self.osc_kind = OscKind::Osc5522;
                        self.state = State::Body;
                        self.buf.clear();
                    } else if PREFIX_52.starts_with(&self.buf) || PREFIX_5522.starts_with(&self.buf)
                    {
                        // Wait for remaining prefix bytes.
                    } else {
                        // 不是 OSC 52/5522:原样放行。最后一个字节若是 ESC,它可能是下一个
                        // 序列的开头,交回 Esc 状态处理。
                        out.extend_from_slice(b"\x1b]");
                        if b == ESC {
                            out.extend_from_slice(&self.buf[..n - 1]);
                            self.state = State::Esc;
                        } else {
                            out.extend_from_slice(&self.buf);
                            self.state = State::Ground;
                        }
                        self.buf.clear();
                    }
                }
                State::Body => match b {
                    BEL => {
                        self.finish(&mut copies, &mut replies);
                        self.state = State::Ground;
                    }
                    ESC => self.state = State::BodyEsc,
                    CAN | SUB => {
                        self.abort_body();
                        self.state = State::Ground;
                    }
                    _ => {
                        if self.buf.len() >= MAX_PAYLOAD {
                            self.enter_discard();
                        } else {
                            self.buf.push(b);
                        }
                    }
                },
                State::BodyEsc => {
                    if b == b'\\' {
                        self.finish(&mut copies, &mut replies);
                        self.state = State::Ground;
                    } else {
                        // ESC 打断了 OSC:丢掉这条,ESC 当作新序列的开头重新处理 b
                        self.abort_body();
                        self.state = State::Esc;
                        continue;
                    }
                }
                State::Discard => match b {
                    BEL => {
                        self.finish_discard(&mut replies);
                        self.state = State::Ground;
                    }
                    CAN | SUB => {
                        self.abort_discard();
                        self.state = State::Ground;
                    }
                    ESC => self.state = State::DiscardEsc,
                    _ => {}
                },
                State::DiscardEsc => {
                    if b == b'\\' {
                        self.finish_discard(&mut replies);
                        self.state = State::Ground;
                    } else {
                        self.abort_discard();
                        self.state = State::Esc;
                        continue;
                    }
                }
            }
            i += 1;
        }
        FilterResult {
            bytes: out,
            copies,
            replies,
        }
    }

    fn finish(&mut self, copies: &mut Vec<String>, replies: &mut Vec<Vec<u8>>) {
        let payload = std::mem::take(&mut self.buf);
        match self.osc_kind {
            OscKind::Osc52 => {
                if let Some(text) = decode_payload(&payload) {
                    copies.push(text);
                }
            }
            OscKind::Osc5522 => {
                self.finish_kitty(&payload, copies, replies);
            }
        }
    }

    fn abort_body(&mut self) {
        self.buf.clear();
        if self.osc_kind == OscKind::Osc5522 {
            self.kitty_write = None;
            self.discard_meta = None;
        }
    }

    fn enter_discard(&mut self) {
        if self.osc_kind == OscKind::Osc52 {
            tracing::debug!("OSC 52 payload too large, discarding");
        } else {
            tracing::debug!("OSC 5522 payload too large, discarding");
            let raw = std::mem::take(&mut self.buf);
            let meta_bytes = match raw.iter().position(|&b| b == b';') {
                Some(idx) => &raw[..idx],
                None => &raw[..],
            };
            let meta = parse_kitty_meta(meta_bytes);
            match meta.op_type.as_deref() {
                Some("read" | "rdata") => {
                    self.discard_meta = Some(DiscardMeta::Read { id: meta.id });
                }
                Some("write") => {
                    let id = meta.id;
                    self.kitty_write = Some(KittyWriteState {
                        id: id.clone(),
                        overflowed: true,
                        ..KittyWriteState::default()
                    });
                    self.discard_meta = Some(DiscardMeta::Write { id });
                }
                _ => {
                    let id = if let Some(state) = &mut self.kitty_write {
                        state.overflowed = true;
                        state.text_b64.clear();
                        if state.id.is_none() {
                            state.id = meta.id;
                        }
                        state.id.clone()
                    } else {
                        meta.id
                    };
                    self.discard_meta = Some(DiscardMeta::Write { id });
                }
            }
        }
        self.buf = Vec::new();
        self.state = State::Discard;
    }

    fn finish_discard(&mut self, replies: &mut Vec<Vec<u8>>) {
        let meta = self.discard_meta.take();
        if self.osc_kind != OscKind::Osc5522 {
            return;
        }
        match meta {
            Some(DiscardMeta::Read { id }) => {
                replies.push(format_kitty_reply("read", "EPERM", id.as_deref()));
            }
            Some(DiscardMeta::Write { id }) => {
                if let Some(state) = &mut self.kitty_write {
                    if !state.replied {
                        state.replied = true;
                        let reply_id = state.id.as_deref().or(id.as_deref());
                        replies.push(format_kitty_reply("write", "EFBIG", reply_id));
                    }
                } else {
                    replies.push(format_kitty_reply("write", "EFBIG", id.as_deref()));
                }
            }
            None => {}
        }
    }

    fn abort_discard(&mut self) {
        self.discard_meta = None;
        if self.osc_kind == OscKind::Osc5522 {
            self.kitty_write = None;
        }
    }

    fn finish_kitty(&mut self, raw: &[u8], copies: &mut Vec<String>, replies: &mut Vec<Vec<u8>>) {
        let (meta_bytes, payload_opt) = match raw.iter().position(|&b| b == b';') {
            Some(idx) => (&raw[..idx], Some(&raw[idx + 1..])),
            None => (raw, None),
        };
        let meta = parse_kitty_meta(meta_bytes);
        match meta.op_type.as_deref() {
            Some("read" | "rdata") => {
                replies.push(format_kitty_reply("read", "EPERM", meta.id.as_deref()));
            }
            Some("write") => {
                let mut state = KittyWriteState {
                    id: meta.id,
                    ..KittyWriteState::default()
                };
                if let Some(mime_b64) = meta.mime_b64.as_deref() {
                    update_kitty_mime(&mut state, mime_b64);
                }
                if let Some(payload) = payload_opt
                    && !payload.is_empty()
                {
                    append_kitty_payload(&mut state, payload);
                    finalize_kitty_write(state, copies, replies);
                    self.kitty_write = None;
                } else {
                    self.kitty_write = Some(state);
                }
            }
            Some("wdata") => {
                let Some(mut state) = self.kitty_write.take() else {
                    return;
                };
                if state.id.is_none() {
                    state.id = meta.id;
                }
                let is_terminator =
                    meta.mime_b64.is_none() && payload_opt.is_none_or(|p| p.is_empty());
                if is_terminator {
                    finalize_kitty_write(state, copies, replies);
                    return;
                }
                if let Some(mime_b64) = meta.mime_b64.as_deref() {
                    update_kitty_mime(&mut state, mime_b64);
                }
                if let Some(payload) = payload_opt
                    && !payload.is_empty()
                {
                    append_kitty_payload(&mut state, payload);
                }
                self.kitty_write = Some(state);
            }
            _ => {}
        }
    }
}

struct KittyMeta {
    op_type: Option<String>,
    id: Option<String>,
    mime_b64: Option<Vec<u8>>,
}

fn parse_kitty_meta(meta: &[u8]) -> KittyMeta {
    let mut out = KittyMeta {
        op_type: None,
        id: None,
        mime_b64: None,
    };
    for part in meta.split(|&b| b == b':') {
        let Some(eq) = part.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = &part[..eq];
        let val = &part[eq + 1..];
        match key {
            b"type" => {
                if let Ok(s) = std::str::from_utf8(val) {
                    out.op_type = Some(s.trim().to_string());
                }
            }
            b"id" => out.id = sanitize_id(val),
            b"mime" => out.mime_b64 = Some(val.to_vec()),
            _ => {}
        }
    }
    out
}

/// Sanitize OSC 5522 `id` to printable ASCII excluding `;`, `:`, `\x1b`, `\x07` (max 64 bytes).
fn sanitize_id(raw: &[u8]) -> Option<String> {
    let mut out = String::new();
    for &b in raw {
        if (0x20..=0x7e).contains(&b) && b != b';' && b != b':' {
            out.push(b as char);
            if out.len() >= MAX_KITTY_ID_LEN {
                break;
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn decode_mime_b64(raw: &[u8]) -> Option<String> {
    let cleaned: Vec<u8> = raw
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let bytes = ENGINE.decode(&cleaned).ok()?;
    let mime = String::from_utf8(bytes).ok()?;
    let trimmed = mime.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn is_plain_text_mime(mime: &str) -> bool {
    let trimmed = mime.trim();
    trimmed.eq_ignore_ascii_case("text/plain")
        || trimmed
            .split_once(';')
            .is_some_and(|(media, _)| media.trim_end().eq_ignore_ascii_case("text/plain"))
}

fn update_kitty_mime(state: &mut KittyWriteState, mime_b64: &[u8]) {
    match decode_mime_b64(mime_b64) {
        Some(mime) => state.current_mime = Some(mime),
        None => {
            state.invalid = true;
            state.text_b64.clear();
        }
    }
}

fn append_kitty_payload(state: &mut KittyWriteState, payload: &[u8]) {
    let is_plain = state
        .current_mime
        .as_deref()
        .is_none_or(is_plain_text_mime);
    if !is_plain || state.overflowed || state.invalid {
        return;
    }
    let cleaned: Vec<u8> = payload
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if state.text_b64.len().saturating_add(cleaned.len()) > MAX_PAYLOAD {
        state.overflowed = true;
        state.text_b64.clear();
    } else {
        state.text_b64.extend_from_slice(&cleaned);
    }
}

fn finalize_kitty_write(
    state: KittyWriteState,
    copies: &mut Vec<String>,
    replies: &mut Vec<Vec<u8>>,
) {
    if state.replied {
        return;
    }
    let id = state.id.as_deref();
    if state.overflowed {
        replies.push(format_kitty_reply("write", "EFBIG", id));
        return;
    }
    if state.invalid {
        replies.push(format_kitty_reply("write", "EINVAL", id));
        return;
    }
    if !state.text_b64.is_empty() {
        let Ok(bytes) = ENGINE.decode(&state.text_b64) else {
            replies.push(format_kitty_reply("write", "EINVAL", id));
            return;
        };
        if !bytes.is_empty() {
            copies.push(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    replies.push(format_kitty_reply("write", "DONE", id));
}

fn format_kitty_reply(op_type: &str, status: &str, id: Option<&str>) -> Vec<u8> {
    match id {
        Some(id) => format!("\x1b]5522;type={op_type}:status={status}:id={id}\x1b\\").into_bytes(),
        None => format!("\x1b]5522;type={op_type}:status={status}\x1b\\").into_bytes(),
    }
}

/// `<sel>;<base64>` → 文本。查询(`?`)、清空(空负载)、解不开的一律 `None`。
fn decode_payload(payload: &[u8]) -> Option<String> {
    let semi = payload.iter().position(|&b| b == b';')?;
    let data = &payload[semi + 1..];
    if data.is_empty() || data == b"?" {
        return None;
    }
    let cleaned: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let bytes = ENGINE.decode(&cleaned).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&[u8]]) -> (Vec<u8>, Vec<String>) {
        let mut f = Osc52Filter::new();
        let mut out = Vec::new();
        let mut copies = Vec::new();
        for c in chunks {
            let (o, cp) = f.filter(c);
            out.extend(o);
            copies.extend(cp);
        }
        (out, copies)
    }

    #[test]
    fn strips_bel_and_st_terminated_osc52_and_extracts_text() {
        let (out, copies) = run(&[b"a\x1b]52;c;aGVsbG8=\x07b\x1b]52;;5L2g5aW9\x1b\\c"]);
        assert_eq!(out, b"abc");
        assert_eq!(copies, ["hello", "你好"]);
    }

    #[test]
    fn passes_through_other_escapes_and_oscs_untouched() {
        let input: &[u8] =
            b"\x1b[31mred\x1b[0m\x1b]0;title\x07\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\\x1b]5;x\x07\x1b\x1b[H";
        let (out, copies) = run(&[input]);
        assert_eq!(out, input);
        assert!(copies.is_empty());
    }

    #[test]
    fn reassembles_sequences_split_at_every_byte() {
        let input: &[u8] = b"x\x1b]52;c;aGVsbG8=\x1b\\y\x1b]2;t\x07z";
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let (out, copies) = run(&chunks);
        assert_eq!(out, b"xy\x1b]2;t\x07z");
        assert_eq!(copies, ["hello"]);
    }

    #[test]
    fn drops_queries_and_clears_without_events() {
        let (out, copies) = run(&[b"1\x1b]52;c;?\x072\x1b]52;c;\x073"]);
        assert_eq!(out, b"123");
        assert!(copies.is_empty());
    }

    #[test]
    fn accepts_unpadded_base64() {
        let (_, copies) = run(&[b"\x1b]52;c;aGk\x07"]);
        assert_eq!(copies, ["hi"]);
    }

    #[test]
    fn interrupted_osc52_is_dropped_and_following_escape_survives() {
        let (out, copies) = run(&[b"\x1b]52;c;aGVs\x1b[1mB\x18"]);
        assert_eq!(out, b"\x1b[1mB\x18");
        assert!(copies.is_empty());
        let (out, copies) = run(&[b"\x1b]52;c;aGVs\x18after"]);
        assert_eq!(out, b"after");
        assert!(copies.is_empty());
    }

    #[test]
    fn esc_inside_prefix_starts_a_new_sequence() {
        let (out, copies) = run(&[b"\x1b]5\x1b]52;c;aGk=\x07!"]);
        assert_eq!(out, b"\x1b]5!");
        assert_eq!(copies, ["hi"]);
    }

    #[test]
    fn oversized_payload_is_swallowed_without_leaking() {
        let mut input = b"\x1b]52;c;".to_vec();
        input.extend(std::iter::repeat_n(b'A', MAX_PAYLOAD + 10));
        input.extend_from_slice(b"\x07tail");
        let (out, copies) = run(&[&input]);
        assert_eq!(out, b"tail");
        assert!(copies.is_empty());
    }

    fn run_with_replies(chunks: &[&[u8]]) -> (Vec<u8>, Vec<String>, Vec<Vec<u8>>) {
        let mut f = Osc52Filter::new();
        let mut out = Vec::new();
        let mut copies = Vec::new();
        let mut replies = Vec::new();
        for c in chunks {
            let res = f.filter_with_replies(c);
            out.extend(res.bytes);
            copies.extend(res.copies);
            replies.extend(res.replies);
        }
        (out, copies, replies)
    }

    #[test]
    fn kitty_osc5522_streams_plain_text_write_and_replies_done() {
        let input = b"pre\x1b]5522;type=write:id=k1\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGVs\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;bG8=\x1b\\\x1b]5522;type=wdata\x1b\\post";
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let (out, copies, replies) = run_with_replies(&chunks);
        assert_eq!(out, b"prepost");
        assert_eq!(copies, ["hello"]);
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=DONE:id=k1\x1b\\".to_vec()]);

        // Case-insensitive text/plain;charset=utf-8 and continuation wdata without mime
        let cont = b"\x1b]5522;type=write:id=k2\x07\x1b]5522;type=wdata:mime=VEVYVC9QTEFJTjtjaGFyc2V0PXV0Zi04;aGVs\x07\x1b]5522;type=wdata;bG8=\x07\x1b]5522;type=wdata\x07";
        let (out2, copies2, replies2) = run_with_replies(&[cont]);
        assert!(out2.is_empty());
        assert_eq!(copies2, ["hello"]);
        assert_eq!(replies2, vec![b"\x1b]5522;type=write:status=DONE:id=k2\x1b\\".to_vec()]);
    }

    #[test]
    fn kitty_osc5522_denies_read_queries_with_eperm() {
        let input = b"A\x1b]5522;type=read:id=r1;dGV4dC9wbGFpbg==\x1b\\B\x1b]5522;type=rdata:id=r2;dGV4dC9wbGFpbg==\x1b\\C\x1b]5522;type=read;dGV4dC9wbGFpbg==\x1b\\D";
        let (out, copies, replies) = run_with_replies(&[input]);
        assert_eq!(out, b"ABCD");
        assert!(copies.is_empty());
        assert_eq!(
            replies,
            vec![
                b"\x1b]5522;type=read:status=EPERM:id=r1\x1b\\".to_vec(),
                b"\x1b]5522;type=read:status=EPERM:id=r2\x1b\\".to_vec(),
                b"\x1b]5522;type=read:status=EPERM\x1b\\".to_vec(),
            ]
        );
    }

    #[test]
    fn kitty_osc5522_ignores_non_text_mime_and_rejects_bad_base64_and_oversized() {
        // 1. image/png chunks stripped without delivering text and still acknowledge DONE
        let img = b"x\x1b]5522;type=write:id=img1\x1b\\\x1b]5522;type=wdata:mime=aW1hZ2UvcG5n;iVBORw0K\x1b\\\x1b]5522;type=wdata;gAAA\x1b\\\x1b]5522;type=wdata\x1b\\y";
        let (out, copies, replies) = run_with_replies(&[img]);
        assert_eq!(out, b"xy");
        assert!(copies.is_empty());
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=DONE:id=img1\x1b\\".to_vec()]);

        // 1b. mixed image/png + text/plain delivers only text/plain and acknowledges DONE
        let mixed = b"\x1b]5522;type=write:id=mix1\x1b\\\x1b]5522;type=wdata:mime=aW1hZ2UvcG5n;iVBORw0K\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGVsbG8=\x1b\\\x1b]5522;type=wdata\x1b\\";
        let (out, copies, replies) = run_with_replies(&[mixed]);
        assert!(out.is_empty());
        assert_eq!(copies, ["hello"]);
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=DONE:id=mix1\x1b\\".to_vec()]);

        // 2. invalid base64 payload replies status=EINVAL and discards write
        let bad = b"1\x1b]5522;type=write:id=bad1\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;!!!bad!!!\x1b\\\x1b]5522;type=wdata\x1b\\2";
        let (out, copies, replies) = run_with_replies(&[bad]);
        assert_eq!(out, b"12");
        assert!(copies.is_empty());
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=EINVAL:id=bad1\x1b\\".to_vec()]);

        // 2b. invalid base64 in mime= also replies status=EINVAL and discards write
        let bad_mime = b"\x1b]5522;type=write:id=badm\x1b\\\x1b]5522;type=wdata:mime=!!!;aGVsbG8=\x1b\\\x1b]5522;type=wdata\x1b\\";
        let (out, copies, replies) = run_with_replies(&[bad_mime]);
        assert!(out.is_empty());
        assert!(copies.is_empty());
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=EINVAL:id=badm\x1b\\".to_vec()]);

        // 3a. multi-chunk payload exceeding MAX_PAYLOAD replies status=EFBIG and discards write
        let half_plus = vec![b'A'; MAX_PAYLOAD / 2 + 16];
        let mut chunk1 = b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;".to_vec();
        chunk1.extend_from_slice(&half_plus);
        chunk1.extend_from_slice(b"\x1b\\");
        let (out, copies, replies) = run_with_replies(&[
            b"h\x1b]5522;type=write:id=big1\x1b\\",
            &chunk1,
            &chunk1,
            b"\x1b]5522;type=wdata\x1b\\t",
        ]);
        assert_eq!(out, b"ht");
        assert!(copies.is_empty());
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=EFBIG:id=big1\x1b\\".to_vec()]);

        // 3b. single OSC 5522 frame exceeding MAX_PAYLOAD enters Discard and replies EFBIG without leaking
        let mut single_huge = b"head\x1b]5522;type=write:id=big2\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;".to_vec();
        single_huge.extend(std::iter::repeat_n(b'A', MAX_PAYLOAD + 10));
        single_huge.extend_from_slice(b"\x1b\\\x1b]5522;type=wdata\x1b\\tail");
        let (out, copies, replies) = run_with_replies(&[&single_huge]);
        assert_eq!(out, b"headtail");
        assert!(copies.is_empty());
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=EFBIG:id=big2\x1b\\".to_vec()]);

        // 3c. standalone oversized single-packet type=write also replies EFBIG without leaking
        let mut standalone_huge = b"s\x1b]5522;type=write:id=big3:mime=dGV4dC9wbGFpbg==;".to_vec();
        standalone_huge.extend(std::iter::repeat_n(b'A', MAX_PAYLOAD + 10));
        standalone_huge.extend_from_slice(b"\x1b\\e");
        let (out, copies, replies) = run_with_replies(&[&standalone_huge]);
        assert_eq!(out, b"se");
        assert!(copies.is_empty());
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=EFBIG:id=big3\x1b\\".to_vec()]);
    }

    #[test]
    fn prefix_matching_preserves_similar_oscs_across_byte_splits() {
        let input = b"\x1b]5;x\x07\x1b]55;y\x07\x1b]552;z\x07\x1b]55220;w\x07\x1b]520;q\x07\x1b]552\x1b]5522;type=write:id=s1\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGk=\x1b\\\x1b]5522;type=wdata\x1b\\!";
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let (out, copies, replies) = run_with_replies(&chunks);
        assert_eq!(out, b"\x1b]5;x\x07\x1b]55;y\x07\x1b]552;z\x07\x1b]55220;w\x07\x1b]520;q\x07\x1b]552!");
        assert_eq!(copies, ["hi"]);
        assert_eq!(replies, vec![b"\x1b]5522;type=write:status=DONE:id=s1\x1b\\".to_vec()]);
    }

    #[test]
    fn sanitizes_kitty_id_to_printable_ascii_max_64_bytes() {
        let mut raw_id = b"ab\nc\r\x7f".to_vec();
        raw_id.extend_from_slice("é".as_bytes());
        raw_id.extend(std::iter::repeat_n(b'z', 80));
        let mut seq = b"\x1b]5522;type=read:id=".to_vec();
        seq.extend_from_slice(&raw_id);
        seq.extend_from_slice(b";dGV4dC9wbGFpbg==\x1b\\");
        let (out, copies, replies) = run_with_replies(&[&seq]);
        assert!(out.is_empty());
        assert!(copies.is_empty());
        let expected_id = format!("abc{}", "z".repeat(61));
        assert_eq!(expected_id.len(), 64);
        assert_eq!(
            replies,
            vec![format!("\x1b]5522;type=read:status=EPERM:id={expected_id}\x1b\\").into_bytes()]
        );
    }
}
