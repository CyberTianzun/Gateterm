//! Named-key and bracketed-paste input encoder for agent/HTTP session control.

use std::borrow::Cow;

use super::grid::InputModes;

pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_KEYS: usize = 64;

const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";
const WIN32_SHIFT_ENTER: &[u8] = b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_";

/// Encode optional text, named keys, and optional trailing Enter into PTY bytes.
pub fn encode_text_and_keys(
    text: Option<&str>,
    keys: &[String],
    enter: bool,
    modes: InputModes,
    win32_input: bool,
) -> Result<Vec<u8>, String> {
    let has_text = text.is_some_and(|t| !t.is_empty());
    if !has_text && keys.is_empty() && !enter {
        return Err("at least one of non-empty text, keys, or enter=true is required".into());
    }
    if let Some(t) = text
        && t.len() > MAX_TEXT_BYTES
    {
        return Err(format!("text exceeds maximum size of {MAX_TEXT_BYTES} bytes"));
    }
    if keys.len() > MAX_KEYS {
        return Err(format!("keys exceeds maximum count of {MAX_KEYS} items"));
    }

    let mut out = Vec::new();
    if let Some(t) = text
        && !t.is_empty()
    {
        if modes.bracketed_paste && (t.contains('\n') || t.contains('\r')) {
            out.extend_from_slice(BRACKETED_PASTE_START);
            out.extend_from_slice(strip_paste_end(t).as_bytes());
            out.extend_from_slice(BRACKETED_PASTE_END.as_bytes());
        } else {
            out.extend_from_slice(t.as_bytes());
        }
    }

    for key in keys {
        out.extend_from_slice(&encode_named_key(key, modes, win32_input)?);
    }

    if enter {
        out.push(b'\r');
    }

    Ok(out)
}

/// Encode a single whitelisted named key using the session's active input modes.
pub fn encode_named_key(
    raw: &str,
    modes: InputModes,
    win32_input: bool,
) -> Result<Vec<u8>, String> {
    if raw.is_empty() {
        return Err("empty key name".into());
    }
    let (modifier, key) = split_modifier(raw);
    match modifier {
        None => encode_unmodified_key(key, modes),
        Some(Modifier::Shift) => encode_shift_key(key, modes, win32_input),
        Some(Modifier::Ctrl) => encode_ctrl_key(key),
        Some(Modifier::Alt) => encode_alt_key(key),
    }
    .ok_or_else(|| format!("unsupported key name: {raw:?}"))
}

/// Strip `\x1b[201~` iteratively so nested sequences cannot reform a closing delimiter.
fn strip_paste_end(text: &str) -> Cow<'_, str> {
    const END: &[u8] = BRACKETED_PASTE_END.as_bytes();
    if !text.contains(BRACKETED_PASTE_END) {
        return Cow::Borrowed(text);
    }
    let mut out = Vec::with_capacity(text.len());
    for &b in text.as_bytes() {
        out.push(b);
        if out.ends_with(END) {
            out.truncate(out.len() - END.len());
        }
    }
    // `END` is pure ASCII, so dropping complete `END` slices never splits UTF-8 codepoints.
    Cow::Owned(String::from_utf8(out).expect("stripping ASCII escape preserves UTF-8"))
}

#[derive(Clone, Copy)]
enum Modifier {
    Ctrl,
    Shift,
    Alt,
}

fn split_modifier(raw: &str) -> (Option<Modifier>, &str) {
    let bytes = raw.as_bytes();
    for (prefix, modifier) in [
        (b"ctrl".as_slice(), Modifier::Ctrl),
        (b"shift".as_slice(), Modifier::Shift),
        (b"alt".as_slice(), Modifier::Alt),
    ] {
        let len = prefix.len();
        if bytes.len() > len
            && bytes[..len].eq_ignore_ascii_case(prefix)
            && matches!(bytes[len], b'+' | b'-')
        {
            return (Some(modifier), &raw[len + 1..]);
        }
    }
    (None, raw)
}

fn encode_unmodified_key(key: &str, modes: InputModes) -> Option<Vec<u8>> {
    if !key.is_ascii() {
        return None;
    }
    let seq: &[u8] = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => b"\r",
        "tab" => b"\t",
        "backspace" | "bs" => b"\x7f",
        "escape" | "esc" => b"\x1b",
        "space" => b" ",
        "delete" | "del" => b"\x1b[3~",
        "insert" | "ins" => b"\x1b[2~",
        "pageup" | "pgup" => b"\x1b[5~",
        "pagedown" | "pgdn" => b"\x1b[6~",
        "home" => {
            if modes.app_cursor {
                b"\x1bOH"
            } else {
                b"\x1b[H"
            }
        }
        "end" => {
            if modes.app_cursor {
                b"\x1bOF"
            } else {
                b"\x1b[F"
            }
        }
        "up" => {
            if modes.app_cursor {
                b"\x1bOA"
            } else {
                b"\x1b[A"
            }
        }
        "down" => {
            if modes.app_cursor {
                b"\x1bOB"
            } else {
                b"\x1b[B"
            }
        }
        "right" => {
            if modes.app_cursor {
                b"\x1bOC"
            } else {
                b"\x1b[C"
            }
        }
        "left" => {
            if modes.app_cursor {
                b"\x1bOD"
            } else {
                b"\x1b[D"
            }
        }
        "f1" => b"\x1bOP",
        "f2" => b"\x1bOQ",
        "f3" => b"\x1bOR",
        "f4" => b"\x1bOS",
        "f5" => b"\x1b[15~",
        "f6" => b"\x1b[17~",
        "f7" => b"\x1b[18~",
        "f8" => b"\x1b[19~",
        "f9" => b"\x1b[20~",
        "f10" => b"\x1b[21~",
        "f11" => b"\x1b[23~",
        "f12" => b"\x1b[24~",
        _ => return None,
    };
    Some(seq.to_vec())
}

fn encode_shift_key(key: &str, modes: InputModes, win32_input: bool) -> Option<Vec<u8>> {
    if !key.is_ascii() {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "tab" => Some(b"\x1b[Z".to_vec()),
        "enter" | "return" => Some(
            if win32_input {
                WIN32_SHIFT_ENTER.to_vec()
            } else if modes.modify_other_keys > 0 {
                b"\x1b[27;2;13~".to_vec()
            } else {
                b"\n".to_vec()
            },
        ),
        _ => None,
    }
}

fn encode_ctrl_key(key: &str) -> Option<Vec<u8>> {
    if key.eq_ignore_ascii_case("space") {
        return Some(vec![0x00]);
    }
    if key.len() != 1 {
        return None;
    }
    let byte = match key.as_bytes()[0] {
        b @ (b'a'..=b'z' | b'A'..=b'Z') => b.to_ascii_lowercase() - b'a' + 1,
        b'@' => 0x00,
        b'[' => 0x1b,
        b'\\' => 0x1c,
        b']' => 0x1d,
        b'^' => 0x1e,
        b'_' => 0x1f,
        _ => return None,
    };
    Some(vec![byte])
}

fn encode_alt_key(key: &str) -> Option<Vec<u8>> {
    if !key.is_ascii() {
        return None;
    }
    let seq: &[u8] = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => b"\x1b\r",
        "backspace" | "bs" => b"\x1b\x7f",
        "up" => b"\x1b[1;3A",
        "down" => b"\x1b[1;3B",
        "right" => b"\x1b[1;3C",
        "left" => b"\x1b[1;3D",
        "space" => b"\x1b ",
        _ => {
            if key.len() == 1 {
                let b = key.as_bytes()[0];
                if (0x20..=0x7e).contains(&b) {
                    return Some(vec![0x1b, b]);
                }
            }
            return None;
        }
    };
    Some(seq.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::grid::InputModes;

    #[test]
    fn encodes_named_keys_and_modifiers_across_terminal_modes() {
        let normal = InputModes::default();
        let app_cursor = InputModes {
            app_cursor: true,
            ..Default::default()
        };
        let mod_other = InputModes {
            modify_other_keys: 1,
            ..Default::default()
        };

        for (raw, expected) in [
            ("Enter", b"\r".as_slice()),
            ("Return", b"\r"),
            ("Tab", b"\t"),
            ("Shift+Tab", b"\x1b[Z"),
            ("shift-tab", b"\x1b[Z"),
            ("Backspace", b"\x7f"),
            ("BS", b"\x7f"),
            ("Escape", b"\x1b"),
            ("Esc", b"\x1b"),
            ("Space", b" "),
            ("Delete", b"\x1b[3~"),
            ("Del", b"\x1b[3~"),
            ("Insert", b"\x1b[2~"),
            ("Ins", b"\x1b[2~"),
            ("PageUp", b"\x1b[5~"),
            ("PgUp", b"\x1b[5~"),
            ("PageDown", b"\x1b[6~"),
            ("PgDn", b"\x1b[6~"),
            ("Home", b"\x1b[H"),
            ("End", b"\x1b[F"),
            ("Up", b"\x1b[A"),
            ("Down", b"\x1b[B"),
            ("Left", b"\x1b[D"),
            ("Right", b"\x1b[C"),
        ] {
            assert_eq!(encode_named_key(raw, normal, false).unwrap(), expected, "{raw}");
        }

        for (raw, expected) in [
            ("Home", b"\x1bOH".as_slice()),
            ("End", b"\x1bOF"),
            ("Up", b"\x1bOA"),
            ("Down", b"\x1bOB"),
            ("Left", b"\x1bOD"),
            ("Right", b"\x1bOC"),
        ] {
            assert_eq!(
                encode_named_key(raw, app_cursor, false).unwrap(),
                expected,
                "{raw} app_cursor"
            );
        }

        assert_eq!(
            encode_named_key("Shift+Enter", normal, true).unwrap(),
            b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_"
        );
        assert_eq!(
            encode_named_key("Shift+Enter", mod_other, false).unwrap(),
            b"\x1b[27;2;13~"
        );
        assert_eq!(
            encode_named_key("Shift+Enter", normal, false).unwrap(),
            b"\n"
        );
        assert_eq!(
            encode_named_key("shift-return", mod_other, false).unwrap(),
            b"\x1b[27;2;13~"
        );

        for (raw, expected) in [
            ("Ctrl+A", [0x01].as_slice()),
            ("Ctrl+C", &[0x03]),
            ("ctrl-d", &[0x04]),
            ("Ctrl+J", &[0x0a]),
            ("Ctrl+L", &[0x0c]),
            ("Ctrl+Z", &[0x1a]),
            ("Ctrl+Space", &[0x00]),
            ("Ctrl+@", &[0x00]),
            ("Ctrl+[", &[0x1b]),
            ("Ctrl+\\", &[0x1c]),
            ("Ctrl+]", &[0x1d]),
            ("Ctrl+^", &[0x1e]),
            ("Ctrl+_", &[0x1f]),
            ("Alt+Enter", b"\x1b\r"),
            ("Alt+Return", b"\x1b\r"),
            ("Alt+Backspace", b"\x1b\x7f"),
            ("Alt+BS", b"\x1b\x7f"),
            ("Alt+Up", b"\x1b[1;3A"),
            ("Alt+Down", b"\x1b[1;3B"),
            ("Alt+Left", b"\x1b[1;3D"),
            ("Alt+Right", b"\x1b[1;3C"),
            ("Alt+x", b"\x1bx"),
            ("Alt+X", b"\x1bX"),
            ("Alt+-", b"\x1b-"),
            ("Alt++", b"\x1b+"),
            ("F1", b"\x1bOP"),
            ("F2", b"\x1bOQ"),
            ("F3", b"\x1bOR"),
            ("F4", b"\x1bOS"),
            ("F5", b"\x1b[15~"),
            ("F6", b"\x1b[17~"),
            ("F7", b"\x1b[18~"),
            ("F8", b"\x1b[19~"),
            ("F9", b"\x1b[20~"),
            ("F10", b"\x1b[21~"),
            ("F11", b"\x1b[23~"),
            ("F12", b"\x1b[24~"),
        ] {
            assert_eq!(encode_named_key(raw, normal, false).unwrap(), expected, "{raw}");
        }
    }

    #[test]
    fn wraps_multiline_text_in_bracketed_paste_and_strips_closing_escape() {
        let bp = InputModes {
            bracketed_paste: true,
            ..Default::default()
        };
        let plain = InputModes::default();

        assert_eq!(
            encode_text_and_keys(Some("line1\nfake\x1b[201~end"), &[], false, bp, false).unwrap(),
            b"\x1b[200~line1\nfakeend\x1b[201~"
        );
        assert_eq!(
            encode_text_and_keys(Some("line1\r\x1b[20\x1b[201~1~end"), &[], false, bp, false)
                .unwrap(),
            b"\x1b[200~line1\rend\x1b[201~"
        );
        assert_eq!(
            encode_text_and_keys(Some("line1\nfake\x1b[201~end"), &[], true, bp, false).unwrap(),
            b"\x1b[200~line1\nfakeend\x1b[201~\r"
        );
        assert_eq!(
            encode_text_and_keys(Some("single line"), &[], false, bp, false).unwrap(),
            b"single line"
        );
        assert_eq!(
            encode_text_and_keys(Some("line1\nfake\x1b[201~end"), &[], false, plain, false)
                .unwrap(),
            b"line1\nfake\x1b[201~end"
        );
        assert_eq!(
            encode_text_and_keys(
                Some("echo"),
                &["Space".into(), "Ctrl+C".into()],
                true,
                plain,
                false,
            )
            .unwrap(),
            b"echo \x03\r"
        );
    }

    #[test]
    fn rejects_empty_oversized_and_unknown_key_inputs() {
        let modes = InputModes::default();
        assert!(encode_text_and_keys(None, &[], false, modes, false).is_err());
        assert!(encode_text_and_keys(Some(""), &[], false, modes, false).is_err());
        assert_eq!(
            encode_text_and_keys(Some(""), &[], true, modes, false).unwrap(),
            b"\r"
        );

        let max_text = "a".repeat(MAX_TEXT_BYTES);
        assert!(encode_text_and_keys(Some(&max_text), &[], false, modes, false).is_ok());
        let too_big_text = "a".repeat(MAX_TEXT_BYTES + 1);
        assert!(encode_text_and_keys(Some(&too_big_text), &[], false, modes, false).is_err());

        let max_keys = vec!["Enter".to_string(); MAX_KEYS];
        assert!(encode_text_and_keys(None, &max_keys, false, modes, false).is_ok());
        let too_many_keys = vec!["Enter".to_string(); MAX_KEYS + 1];
        assert!(encode_text_and_keys(None, &too_many_keys, false, modes, false).is_err());

        for bad in [
            "Ctrl+Bogus",
            "",
            " ",
            "Shift+A",
            "Ctrl+Alt+C",
            "Ctrl+1",
            "Alt+Tab",
            "Alt+\x01",
            "Alt+é",
            "F13",
            "Enter ",
        ] {
            assert!(
                encode_text_and_keys(None, &[bad.to_string()], false, modes, false).is_err(),
                "expected error for {bad:?}"
            );
        }
    }
}
