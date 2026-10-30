//! The unfinished parser input that must follow a terminal snapshot repaint.
//!
//! This mirrors vte 0.15's states because its public parser API does not expose
//! continuation state. Executed controls are excluded from the replay. Ignored
//! strings and DCS payload are compacted; Processor does not handle DCS actions.
//! Parameters use vte's u16 saturation and 32-parameter limit. Only unfinished
//! OSC content can grow with input: retaining its content is necessary to finish
//! the original command without silently truncating a title, clipboard, etc.

#[derive(Clone, Copy, Default)]
enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsIgnore,
    DcsPassthrough,
    Osc,
    IgnoredString,
}

#[derive(Default)]
pub(super) struct StreamTail {
    bytes: Vec<u8>,
    state: State,
    parameters: usize,
    intermediates: usize,
}

impl StreamTail {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.advance_byte(byte);
        }
    }

    pub(super) fn pending(&self) -> &[u8] {
        &self.bytes
    }

    fn reset(&mut self, state: State, bytes: &[u8]) {
        self.state = state;
        self.bytes.clear();
        self.bytes.extend_from_slice(bytes);
        self.parameters = 0;
        self.intermediates = 0;
    }

    fn advance_byte(&mut self, byte: u8) {
        use State::*;

        if matches!(self.state, Ground) {
            self.advance_ground(byte);
            return;
        }

        // These transitions apply in every non-ground vte state. An ESC that
        // ends OSC/DCS has already dispatched the string and starts a new escape.
        match byte {
            0x18 | 0x1a => {
                self.reset(Ground, b"");
                return;
            }
            0x1b => {
                self.reset(Escape, b"\x1b");
                return;
            }
            0x00..=0x1f => {
                if byte == 0x07 && matches!(self.state, Osc) {
                    self.reset(Ground, b"");
                }
                return;
            }
            _ => (),
        }

        match self.state {
            Escape => match byte {
                0x20..=0x2f => {
                    self.collect(byte);
                    self.state = EscapeIntermediate;
                }
                b'P' => self.start(DcsEntry, byte),
                b'[' => self.start(CsiEntry, byte),
                b']' => self.start(Osc, byte),
                b'X' | b'^' | b'_' => self.reset(IgnoredString, b"\x1bX"),
                0x30..=0x7e => self.reset(Ground, b""),
                _ => (),
            },
            EscapeIntermediate => match byte {
                0x20..=0x2f => self.collect(byte),
                0x30..=0x7e => self.reset(Ground, b""),
                _ => (),
            },
            CsiEntry | DcsEntry => match byte {
                0x20..=0x2f => {
                    self.collect(byte);
                    self.state = if matches!(self.state, CsiEntry) {
                        CsiIntermediate
                    } else {
                        DcsIntermediate
                    };
                }
                0x30..=0x3b => {
                    self.parameter(byte);
                    self.state = if matches!(self.state, CsiEntry) {
                        CsiParam
                    } else {
                        DcsParam
                    };
                }
                0x3c..=0x3f => {
                    self.collect(byte);
                    self.state = if matches!(self.state, CsiEntry) {
                        CsiParam
                    } else {
                        DcsParam
                    };
                }
                0x40..=0x7e => self.dispatch(),
                _ => (),
            },
            CsiParam | DcsParam => match byte {
                0x20..=0x2f => {
                    self.collect(byte);
                    self.state = if matches!(self.state, CsiParam) {
                        CsiIntermediate
                    } else {
                        DcsIntermediate
                    };
                }
                0x30..=0x3b => self.parameter(byte),
                0x3c..=0x3f => self.ignore(),
                0x40..=0x7e => self.dispatch(),
                _ => (),
            },
            CsiIntermediate | DcsIntermediate => match byte {
                0x20..=0x2f => self.collect(byte),
                0x30..=0x3f => self.ignore(),
                0x40..=0x7e => self.dispatch(),
                _ => (),
            },
            CsiIgnore if (0x40..=0x7e).contains(&byte) => self.reset(Ground, b""),
            DcsPassthrough if byte == 0x9c => self.reset(Ground, b""),
            Osc => self.bytes.push(byte),
            Ground | CsiIgnore | DcsIgnore | DcsPassthrough | IgnoredString => (),
        }
    }

    fn start(&mut self, state: State, byte: u8) {
        self.state = state;
        self.bytes.push(byte);
    }

    fn dispatch(&mut self) {
        if matches!(
            self.state,
            State::DcsEntry | State::DcsParam | State::DcsIntermediate
        ) {
            self.reset(State::DcsPassthrough, b"\x1bPq");
        } else {
            self.reset(State::Ground, b"");
        }
    }

    fn ignore(&mut self) {
        if matches!(self.state, State::CsiParam | State::CsiIntermediate) {
            self.reset(State::CsiIgnore, b"\x1b[0?");
        } else {
            self.reset(State::DcsIgnore, b"\x1bP0?");
        }
    }

    fn collect(&mut self, byte: u8) {
        // The third intermediate sets vte's ignore flag; later ones do nothing.
        if self.intermediates < 3 {
            self.bytes.push(byte);
            self.intermediates += 1;
        }
    }

    fn parameter(&mut self, byte: u8) {
        if self.parameters == 32 {
            return;
        }
        if byte == b';' || byte == b':' {
            self.parameters += 1;
            self.bytes.push(byte);
        } else {
            let start = self
                .bytes
                .iter()
                .rposition(|b| !b.is_ascii_digit())
                .map_or(0, |i| i + 1);
            self.bytes.push(byte);
            if self.bytes.len() - start > 5 {
                let value = self.bytes[start..].iter().fold(0_u16, |n, b| {
                    n.saturating_mul(10).saturating_add((b - b'0') as u16)
                });
                self.bytes.truncate(start);
                self.bytes.extend_from_slice(value.to_string().as_bytes());
            }
        }
    }

    fn advance_ground(&mut self, byte: u8) {
        if self.bytes.is_empty() {
            if byte == 0x1b {
                self.reset(State::Escape, b"\x1b");
            } else if (0xc2..=0xf4).contains(&byte) {
                self.bytes.push(byte);
            }
            return;
        }

        self.bytes.push(byte);
        match std::str::from_utf8(&self.bytes) {
            Ok(_) => self.bytes.clear(),
            Err(error) => {
                if let Some(invalid_len) = error.error_len() {
                    // The invalid prefix has already printed a replacement.
                    // Reprocess the byte after it, which may start a new escape
                    // or another incomplete UTF-8 character.
                    let remaining = self.bytes[invalid_len..].to_vec();
                    self.bytes.clear();
                    self.advance(&remaining);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StreamTail;
    use alacritty_terminal::vte::{Params, Parser, Perform};

    #[test]
    fn retains_only_unfinished_utf8_or_escape_input() {
        let cases: &[(&[u8], &[u8])] = &[
            (b"plain text", b""),
            (b"abc\xe4\xbd", b"\xe4\xbd"),
            ("abc你".as_bytes(), b""),
            (b"abc\x1b", b"\x1b"),
            (b"abc\x1b[31", b"\x1b[31"),
            (b"abc\x1b[31m", b""),
            (b"\x1b(0", b""),
            (b"\x1b(", b"\x1b("),
            (b"\x1b]2;title", b"\x1b]2;title"),
            (b"\x1b]2;title\x07", b""),
            (b"\x1b]2;title\x1b", b"\x1b"),
            (b"\x1bP1$qhello\x1b", b"\x1b"),
            (b"\x1bP1$qhello\x9c", b""),
            (b"\x1b[123\x18", b""),
            (b"\x1b]title\x1a", b""),
            (b"\xe4\x1b[", b"\x1b["),
            (b"\xe4\xbd\xff", b""),
            (b"\xe4\xbd\xe5", b"\xe5"),
            (b"\x1b[1\n;2\r", b"\x1b[1;2"),
            (b"\x1b\x08(\x0e", b"\x1b("),
        ];
        for &(input, expected) in cases {
            let mut tail = StreamTail::new();
            for byte in input {
                tail.advance(&[*byte]);
            }
            assert_eq!(tail.pending(), expected, "input: {input:?}");
        }
    }

    #[derive(Default, Debug, PartialEq)]
    struct Actions(Vec<String>);

    // DCS hook/put/unhook are deliberately omitted: Processor treats these as
    // unhandled, so only parser state (not already consumed payload) matters.
    impl Perform for Actions {
        fn print(&mut self, c: char) {
            self.0.push(format!("print {c:?}"));
        }

        fn execute(&mut self, byte: u8) {
            self.0.push(format!("execute {byte}"));
        }

        fn csi_dispatch(&mut self, p: &Params, i: &[u8], ignore: bool, c: char) {
            self.0.push(format!("csi {p:?} {i:?} {ignore} {c:?}"));
        }

        fn esc_dispatch(&mut self, i: &[u8], ignore: bool, byte: u8) {
            self.0.push(format!("esc {i:?} {ignore} {byte}"));
        }

        fn osc_dispatch(&mut self, p: &[&[u8]], bell: bool) {
            self.0.push(format!("osc {p:?} {bell}"));
        }
    }

    fn assert_continuation(input: &[u8], split: usize) {
        let mut original = Parser::new();
        let mut actions = Actions::default();
        original.advance(&mut actions, &input[..split]);
        actions.0.clear();
        original.advance(&mut actions, &input[split..]);

        let mut tail = StreamTail::default();
        tail.advance(&input[..split]);
        let mut restored = Parser::new();
        let mut restored_actions = Actions::default();
        restored.advance(&mut restored_actions, tail.pending());
        assert!(
            restored_actions.0.is_empty(),
            "replayed side effects at {split}: {:?}, tail: {:?}",
            restored_actions,
            tail.pending()
        );
        restored.advance(&mut restored_actions, &input[split..]);
        assert_eq!(
            restored_actions,
            actions,
            "split: {split}, input prefix: {:?}, tail: {:?}",
            &input[..input.len().min(64)],
            tail.pending()
        );
    }

    #[test]
    fn restores_actual_parser_continuation_at_every_sequence_boundary() {
        let inputs: &[&[u8]] = &[
            "你好😀".as_bytes(),
            b"\xe4\xbd\xff\xe5\xa5\xbd\xf0\x9f\x98\x80",
            b"\x1b[1\n;2\rHtext\x1b[31mred\x1b[m",
            b"\x1b\x08(\x0e0text\x1b(B",
            b"\x1b[?1;2:3h\x1b[ 1mtext\x1b[123?m",
            b"\x1b]2;hello\x07text\x1b]2;world\x1b\\text",
            b"\x1b]2;hello\x18text\x1b]2;world\x1aafter",
            b"\x1bP1;2$qpayload\x1b\\after\x1bPbad\x18text",
            b"\x1bP 1malformed\x1b\\after\x1bPqdata\x9cafter",
            b"\x1bXignored\x07text\x1b\\after\x1b^ignored\x18text\x1b_ignored\x1aafter",
            b"\x1b]2;a\x00b\x08c\x1b[31mred",
            b"\xe4\xbd\x1b[31mred\xff\x80\xc2\x9c",
        ];
        for input in inputs {
            for split in 0..=input.len() {
                assert_continuation(input, split);
            }
        }
    }

    #[test]
    fn discards_consumed_string_payload_without_losing_string_state() {
        for introducer in [
            b"\x1bX".as_slice(),
            b"\x1b^",
            b"\x1b_",
            b"\x1bPq",
            b"\x1bP 1",
        ] {
            let mut tail = StreamTail::new();
            tail.advance(introducer);
            for _ in 0..1000 {
                tail.advance(&[b'x'; 1024]);
            }
            assert!(
                tail.pending().len() < 16,
                "pending grew: {}",
                tail.pending().len()
            );
            tail.advance(b"\x1b");
            assert_eq!(tail.pending(), b"\x1b");
            tail.advance(b"\\");
            assert!(tail.pending().is_empty());
        }
    }

    #[test]
    fn bounds_parameter_and_intermediate_repetition_without_changing_dispatch() {
        for (prefix, repeated) in [
            (b"\x1b[".as_slice(), b'9'),
            (b"\x1b[", b';'),
            (b"\x1b[", b' '),
            (b"\x1b", b'('),
        ] {
            let mut input = prefix.to_vec();
            input.extend(std::iter::repeat_n(repeated, 100_000));
            let split = input.len();
            input.extend_from_slice(b"mtext");
            let mut tail = StreamTail::new();
            tail.advance(&input[..split]);
            assert!(tail.pending().len() < 256);
            assert_continuation(&input, split);
        }
    }

    #[test]
    fn continuation_matches_actual_parser_for_arbitrary_bytes_and_fragmentation() {
        let alphabet = [
            0, 7, 8, 10, 13, 14, 24, 26, 27, 31, 32, 40, 48, 49, 58, 59, 63, 65, 80, 88, 91, 92,
            93, 94, 95, 97, 126, 127, 128, 156, 191, 194, 224, 237, 240, 244, 255,
        ];
        let mut seed = 0x354a_9832_u32;
        for _ in 0..300 {
            let mut input = vec![0; 40];
            for byte in &mut input {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *byte = alphabet[seed as usize % alphabet.len()];
            }
            let mut fragmented = StreamTail::new();
            for split in 0..=input.len() {
                let mut whole = StreamTail::new();
                whole.advance(&input[..split]);
                assert_eq!(
                    fragmented.pending(),
                    whole.pending(),
                    "fragmentation at {split}: {input:?}"
                );
                assert_continuation(&input, split);
                if split < input.len() {
                    fragmented.advance(&input[split..split + 1]);
                }
            }
        }
    }
}
