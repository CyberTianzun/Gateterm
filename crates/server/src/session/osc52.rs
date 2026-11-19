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
const PREFIX: &[u8] = b"52;";

/// 一条 OSC 52 负载(`<sel>;<base64>`)的上限。超过的整条丢弃,不往外漏字节。
/// 2 MiB base64 ≈ 1.5 MiB 文本;真正投递时还有 1 MiB 文本上限
/// (`pty::MAX_CLIPBOARD_TEXT`)。各家 agent 自己的上限是 100 KB 量级。
pub const MAX_PAYLOAD: usize = 2 * 1024 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    /// 看到 ESC,还不知道是不是 OSC
    Esc,
    /// `ESC ]` 之后,正在比对 `52;`
    Prefix,
    /// OSC 52 负载
    Body,
    /// 负载里看到 ESC,等 `\`
    BodyEsc,
    /// 超长的 OSC 52:吞到结束符为止,不产出事件
    Discard,
    DiscardEsc,
}

#[derive(Default)]
pub struct Osc52Filter {
    state: State,
    /// Prefix 状态下是 `ESC ]` 之后已读的字节;Body 状态下是负载。
    buf: Vec<u8>,
}

impl Osc52Filter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 过滤一段输出:返回去掉 OSC 52 之后的字节,以及其间解出的待复制文本。
    ///
    /// 跨批次切开的序列会被正确拼接 —— 没走完的前缀先扣在手里,下一批再决定放行
    /// 还是吞掉。
    pub fn filter(&mut self, input: &[u8]) -> (Vec<u8>, Vec<String>) {
        let mut out = Vec::with_capacity(input.len());
        let mut copies = Vec::new();
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
                    if self.buf[..] == PREFIX[..n] {
                        if n == PREFIX.len() {
                            self.state = State::Body;
                            self.buf.clear();
                        }
                    } else {
                        // 不是 OSC 52:原样放行。最后一个字节若是 ESC,它可能是下一个
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
                        self.finish(&mut copies);
                        self.state = State::Ground;
                    }
                    ESC => self.state = State::BodyEsc,
                    CAN | SUB => {
                        self.buf.clear();
                        self.state = State::Ground;
                    }
                    _ => {
                        if self.buf.len() >= MAX_PAYLOAD {
                            tracing::debug!("OSC 52 payload too large, discarding");
                            self.buf = Vec::new();
                            self.state = State::Discard;
                        } else {
                            self.buf.push(b);
                        }
                    }
                },
                State::BodyEsc => {
                    if b == b'\\' {
                        self.finish(&mut copies);
                        self.state = State::Ground;
                    } else {
                        // ESC 打断了 OSC:丢掉这条,ESC 当作新序列的开头重新处理 b
                        self.buf.clear();
                        self.state = State::Esc;
                        continue;
                    }
                }
                State::Discard => match b {
                    BEL | CAN | SUB => self.state = State::Ground,
                    ESC => self.state = State::DiscardEsc,
                    _ => {}
                },
                State::DiscardEsc => {
                    if b == b'\\' {
                        self.state = State::Ground;
                    } else {
                        self.state = State::Esc;
                        continue;
                    }
                }
            }
            i += 1;
        }
        (out, copies)
    }

    fn finish(&mut self, copies: &mut Vec<String>) {
        let payload = std::mem::take(&mut self.buf);
        if let Some(text) = decode_payload(&payload) {
            copies.push(text);
        }
    }
}

/// `<sel>;<base64>` → 文本。查询(`?`)、清空(空负载)、解不开的一律 `None`。
fn decode_payload(payload: &[u8]) -> Option<String> {
    let semi = payload.iter().position(|&b| b == b';')?;
    let data = &payload[semi + 1..];
    if data.is_empty() || data == b"?" {
        return None;
    }
    let cleaned: Vec<u8> = data.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    const ENGINE: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
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
}
