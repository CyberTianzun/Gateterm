//! Unified pure-Rust terminal keyboard encoder for Gateterm clients.
//!
//! Priority order (4 tiers):
//! 1. Windows ConPTY `win32-input-mode` (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`) when `server_os == "windows"` and `win32_input_mode == true`.
//! 2. Kitty keyboard protocol (`CSI ... u` / functional keys with event types) when `state.kitty_keyboard_flags > 0`.
//! 3. `modifyOtherKeys` (`CSI 27;mod;code~`) when `state.modify_other_keys > 0`.
//! 4. Legacy VT sequences.

use crate::term::ClientTermState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub key: String,
    pub codepoint: Option<u32>,
    pub shifted_codepoint: Option<u32>,
    pub base_layout_codepoint: Option<u32>,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub super_key: bool,
    pub kind: KeyEventKind,
}

impl KeyInput {
    pub fn simple(
        key: impl Into<String>,
        codepoint: Option<u32>,
        shift: bool,
        alt: bool,
        ctrl: bool,
        kind: KeyEventKind,
    ) -> Self {
        Self {
            key: key.into(),
            codepoint,
            shifted_codepoint: None,
            base_layout_codepoint: None,
            shift,
            alt,
            ctrl,
            super_key: false,
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyInfo {
    pub vk: u32,
    pub scan: u32,
    pub enhanced: bool,
    pub character: u32,
}

pub fn encode_key(
    input: &KeyInput,
    state: &ClientTermState,
    server_os: &str,
    win32_input_mode: bool,
) -> Vec<u8> {
    let key_lower = input.key.to_ascii_lowercase();
    let key = key_lower.as_str();

    // 1. Windows ConPTY `win32-input-mode` (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`)
    if server_os.eq_ignore_ascii_case("windows") && win32_input_mode {
        return encode_win32_key(input, key).unwrap_or_default();
    }

    // Ignore key release events unless Kitty REPORT_EVENT_TYPES (bit 1 = 2) is active.
    if input.kind == KeyEventKind::Release && (state.kitty_keyboard_flags & 2) == 0 {
        return Vec::new();
    }

    // 2. Kitty keyboard protocol (`CSI ... u` / functional keys)
    if state.kitty_keyboard_flags > 0
        && let Some(seq) = encode_kitty_key(input, key, state)
    {
        return seq;
    }

    let mods_mask =
        u32::from(input.shift) | (u32::from(input.alt) << 1) | (u32::from(input.ctrl) << 2);
    let xterm_mod = 1 + mods_mask;

    // 3. XTMODKEYS (`modifyOtherKeys > 0`) for modified Enter, Tab, Backspace, Escape
    if state.kitty_keyboard_flags == 0 && state.modify_other_keys > 0 {
        let other: Option<(u32, &[u32])> = match key {
            "enter" => Some((13, &[0, 2])),
            "tab" => Some((9, &[0, 1, 2])),
            "backspace" => Some((127, &[0, 2, 4])),
            "escape" => Some((27, &[0, 2])),
            _ => None,
        };
        if let Some((code, plain)) = other
            && !plain.contains(&mods_mask)
        {
            return format!("\x1b[27;{xterm_mod};{code}~").into_bytes();
        }
    }

    // 4. Legacy VT: Standard special keys
    match key {
        "enter" => {
            if input.alt {
                return b"\x1b\r".to_vec();
            }
            return b"\r".to_vec();
        }
        "tab" => {
            if input.shift {
                return b"\x1b[Z".to_vec();
            }
            if input.alt {
                return b"\x1b\t".to_vec();
            }
            return b"\t".to_vec();
        }
        "backspace" => {
            let b = if input.ctrl { 0x08 } else { 0x7f };
            if input.alt {
                return vec![0x1b, b];
            }
            return vec![b];
        }
        "escape" => {
            if input.alt {
                return b"\x1b\x1b".to_vec();
            }
            return vec![0x1b];
        }
        "space" | " " => {
            if input.ctrl {
                if input.alt {
                    return vec![0x1b, 0x00];
                }
                return vec![0x00];
            }
            if input.alt {
                return b"\x1b ".to_vec();
            }
            return Vec::new();
        }
        _ => {}
    }

    // Cursor & Navigation keys
    if let Some(seq) = encode_navigation_key(key, xterm_mod, state.application_cursor) {
        return seq;
    }

    // Function keys F1..F12
    if let Some(seq) = encode_function_key(key, xterm_mod) {
        return seq;
    }

    // Control chords (Ctrl+A .. Ctrl+Z, Ctrl+2..8, Ctrl+[, Ctrl+\, Ctrl+], Ctrl+^, Ctrl+_, Ctrl+/)
    if input.ctrl
        && let Some(ctrl_byte) = encode_ctrl_char(input.key.as_str(), input.shift)
    {
        if input.alt {
            return vec![0x1b, ctrl_byte];
        }
        return vec![ctrl_byte];
    }

    // Alt + printable ASCII
    if input.alt && !input.ctrl && input.key.len() == 1 {
        let mut ch = input.key.chars().next().unwrap();
        if ch.is_ascii() {
            if input.shift {
                ch = ch.to_ascii_uppercase();
            }
            return vec![0x1b, ch as u8];
        }
    }

    Vec::new()
}

fn encode_kitty_key(input: &KeyInput, key: &str, state: &ClientTermState) -> Option<Vec<u8>> {
    let flags = state.kitty_keyboard_flags;
    let report_events = (flags & 2) != 0;
    let report_alternate = (flags & 4) != 0;
    let report_all_as_esc = (flags & 8) != 0;
    let report_text = (flags & 16) != 0;

    let mods = 1
        + u32::from(input.shift)
        + (u32::from(input.alt) << 1)
        + (u32::from(input.ctrl) << 2)
        + (u32::from(input.super_key) << 3);

    let event_type: Option<u8> = if report_events {
        match input.kind {
            KeyEventKind::Press => None,
            KeyEventKind::Repeat => Some(2),
            KeyEventKind::Release => Some(3),
        }
    } else {
        None
    };

    // Letter-coded navigation & F1..F4 keys
    let letter = match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    };
    if let Some(l) = letter {
        if let Some(et) = event_type {
            return Some(format!("\x1b[1;{mods}:{et}{l}").into_bytes());
        }
        if mods > 1 {
            return Some(format!("\x1b[1;{mods}{l}").into_bytes());
        }
        // Unmodified press falls through to standard navigation/function key encoding
        return None;
    }

    // Tilde-coded navigation & F5..F12 keys
    let tilde_code = match key {
        "insert" => Some(2u32),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };
    if let Some(code) = tilde_code {
        if let Some(et) = event_type {
            return Some(format!("\x1b[{code};{mods}:{et}~").into_bytes());
        }
        if mods > 1 {
            return Some(format!("\x1b[{code};{mods}~").into_bytes());
        }
        return Some(format!("\x1b[{code}~").into_bytes());
    }

    let is_enter_tab_bs = matches!(key, "enter" | "tab" | "backspace");
    let should_csi_u = report_all_as_esc
        || key == "escape"
        || input.ctrl
        || input.alt
        || input.super_key
        || (is_enter_tab_bs && input.shift)
        || input.kind == KeyEventKind::Release;

    if !should_csi_u {
        return None;
    }

    let cp = match key {
        "escape" => Some(27u32),
        "enter" => Some(13),
        "tab" => Some(9),
        "backspace" => Some(127),
        "space" | " " => Some(32),
        _ => input
            .codepoint
            .filter(|&c| c > 0)
            .map(|c| {
                if u8::try_from(c).is_ok_and(|b| b.is_ascii_uppercase()) {
                    (c as u8).to_ascii_lowercase() as u32
                } else {
                    c
                }
            })
            .or_else(|| {
                if input.key.chars().count() == 1 {
                    let ch = input.key.chars().next().unwrap();
                    Some(if ch.is_ascii_uppercase() {
                        ch.to_ascii_lowercase() as u32
                    } else {
                        ch as u32
                    })
                } else {
                    None
                }
            }),
    }?;

    let key_part = if report_alternate {
        let shifted = input
            .shifted_codepoint
            .or_else(|| {
                if input.shift && u8::try_from(cp).is_ok_and(|b| b.is_ascii_lowercase()) {
                    Some((cp as u8).to_ascii_uppercase() as u32)
                } else {
                    None
                }
            })
            .filter(|&s| s != 0 && s != cp);
        let base = input.base_layout_codepoint.filter(|&b| b != 0 && b != cp);
        match (shifted, base) {
            (Some(s), Some(b)) => format!("{cp}:{s}:{b}"),
            (Some(s), None) => format!("{cp}:{s}"),
            (None, Some(b)) => format!("{cp}::{b}"),
            (None, None) => format!("{cp}"),
        }
    } else {
        format!("{cp}")
    };

    let associated_text = if report_text
        && input.kind != KeyEventKind::Release
        && !input.ctrl
        && !input.alt
        && !input.super_key
        && cp >= 32
        && cp != 127
    {
        let text_cp = if input.shift {
            input.shifted_codepoint.unwrap_or_else(|| {
                if u8::try_from(cp).is_ok_and(|b| b.is_ascii_lowercase()) {
                    (cp as u8).to_ascii_uppercase() as u32
                } else {
                    cp
                }
            })
        } else {
            cp
        };
        Some(text_cp)
    } else {
        None
    };

    let seq = if let Some(text_cp) = associated_text {
        let mods_part = match event_type {
            Some(et) => format!("{mods}:{et}"),
            None if mods > 1 => format!("{mods}"),
            None => String::new(),
        };
        format!("\x1b[{key_part};{mods_part};{text_cp}u")
    } else if let Some(et) = event_type {
        format!("\x1b[{key_part};{mods}:{et}u")
    } else if mods > 1 {
        format!("\x1b[{key_part};{mods}u")
    } else {
        format!("\x1b[{key_part}u")
    };

    Some(seq.into_bytes())
}

fn encode_win32_key(input: &KeyInput, key_lower: &str) -> Option<Vec<u8>> {
    if input.kind == KeyEventKind::Release {
        return None;
    }
    let (info, is_special) = resolve_win32_key(input.key.as_str(), key_lower)?;
    // Unmodified printable characters (including plain space) are committed via IME/text input
    // so IME and dead keys are never broken.
    if !is_special && !input.ctrl && !input.alt && !(key_lower == "space" && input.shift) {
        return None;
    }

    let cp = if info.character != 0 {
        if input.shift && u8::try_from(info.character).is_ok_and(|b| b.is_ascii_lowercase()) {
            (info.character as u8).to_ascii_uppercase() as u32
        } else {
            info.character
        }
    } else {
        0
    };

    let uc = match info.vk {
        13 => {
            if input.ctrl {
                10
            } else {
                13
            }
        }
        9 => 9,
        8 => {
            if input.ctrl {
                127
            } else {
                8
            }
        }
        27 => 27,
        _ => win32_control_character(cp, input.ctrl, input.shift),
    };

    let state = (if info.enhanced { 0x100 } else { 0 })
        | (if input.shift { 0x10 } else { 0 })
        | (if input.ctrl { 0x08 } else { 0 })
        | (if input.alt { 0x02 } else { 0 });

    let seq = format!(
        "\x1b[{};{};{};1;{};1_\x1b[{};{};{};0;{};1_",
        info.vk, info.scan, uc, state, info.vk, info.scan, uc, state
    );
    Some(seq.into_bytes())
}

pub fn resolve_win32_key(raw_key: &str, key_lower: &str) -> Option<(WinKeyInfo, bool)> {
    // 1. Named functional, navigation, and F1..F12 keys (`is_special = true`)
    let special = match key_lower {
        "escape" => Some(WinKeyInfo {
            vk: 27,
            scan: 1,
            enhanced: false,
            character: 27,
        }),
        "backspace" => Some(WinKeyInfo {
            vk: 8,
            scan: 14,
            enhanced: false,
            character: 8,
        }),
        "tab" => Some(WinKeyInfo {
            vk: 9,
            scan: 15,
            enhanced: false,
            character: 9,
        }),
        "enter" => Some(WinKeyInfo {
            vk: 13,
            scan: 28,
            enhanced: false,
            character: 13,
        }),
        "up" => Some(WinKeyInfo {
            vk: 38,
            scan: 72,
            enhanced: true,
            character: 0,
        }),
        "down" => Some(WinKeyInfo {
            vk: 40,
            scan: 80,
            enhanced: true,
            character: 0,
        }),
        "left" => Some(WinKeyInfo {
            vk: 37,
            scan: 75,
            enhanced: true,
            character: 0,
        }),
        "right" => Some(WinKeyInfo {
            vk: 39,
            scan: 77,
            enhanced: true,
            character: 0,
        }),
        "home" => Some(WinKeyInfo {
            vk: 36,
            scan: 71,
            enhanced: true,
            character: 0,
        }),
        "end" => Some(WinKeyInfo {
            vk: 35,
            scan: 79,
            enhanced: true,
            character: 0,
        }),
        "pageup" => Some(WinKeyInfo {
            vk: 33,
            scan: 73,
            enhanced: true,
            character: 0,
        }),
        "pagedown" => Some(WinKeyInfo {
            vk: 34,
            scan: 81,
            enhanced: true,
            character: 0,
        }),
        "insert" => Some(WinKeyInfo {
            vk: 45,
            scan: 82,
            enhanced: true,
            character: 0,
        }),
        "delete" => Some(WinKeyInfo {
            vk: 46,
            scan: 83,
            enhanced: true,
            character: 0,
        }),
        "f1" => Some(WinKeyInfo {
            vk: 112,
            scan: 59,
            enhanced: false,
            character: 0,
        }),
        "f2" => Some(WinKeyInfo {
            vk: 113,
            scan: 60,
            enhanced: false,
            character: 0,
        }),
        "f3" => Some(WinKeyInfo {
            vk: 114,
            scan: 61,
            enhanced: false,
            character: 0,
        }),
        "f4" => Some(WinKeyInfo {
            vk: 115,
            scan: 62,
            enhanced: false,
            character: 0,
        }),
        "f5" => Some(WinKeyInfo {
            vk: 116,
            scan: 63,
            enhanced: false,
            character: 0,
        }),
        "f6" => Some(WinKeyInfo {
            vk: 117,
            scan: 64,
            enhanced: false,
            character: 0,
        }),
        "f7" => Some(WinKeyInfo {
            vk: 118,
            scan: 65,
            enhanced: false,
            character: 0,
        }),
        "f8" => Some(WinKeyInfo {
            vk: 119,
            scan: 66,
            enhanced: false,
            character: 0,
        }),
        "f9" => Some(WinKeyInfo {
            vk: 120,
            scan: 67,
            enhanced: false,
            character: 0,
        }),
        "f10" => Some(WinKeyInfo {
            vk: 121,
            scan: 68,
            enhanced: false,
            character: 0,
        }),
        "f11" => Some(WinKeyInfo {
            vk: 122,
            scan: 87,
            enhanced: false,
            character: 0,
        }),
        "f12" => Some(WinKeyInfo {
            vk: 123,
            scan: 88,
            enhanced: false,
            character: 0,
        }),
        _ => None,
    };
    if let Some(info) = special {
        return Some((info, true));
    }

    if key_lower == "space" || raw_key == " " {
        return Some((
            WinKeyInfo {
                vk: 32,
                scan: 57,
                enhanced: false,
                character: 32,
            },
            false,
        ));
    }

    if raw_key.len() != 1 {
        return None;
    }
    let ch = raw_key.as_bytes()[0];
    if !ch.is_ascii() {
        return None;
    }

    const LETTER_SCANS: [u32; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47,
        17, 45, 21, 44,
    ];
    let lower = ch.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        let idx = (lower - b'a') as usize;
        return Some((
            WinKeyInfo {
                vk: (lower.to_ascii_uppercase()) as u32,
                scan: LETTER_SCANS[idx],
                enhanced: false,
                character: ch as u32,
            },
            false,
        ));
    }

    if ch.is_ascii_digit() {
        let d = (ch - b'0') as u32;
        let scan = if d == 0 { 11 } else { d + 1 };
        return Some((
            WinKeyInfo {
                vk: 48 + d,
                scan,
                enhanced: false,
                character: ch as u32,
            },
            false,
        ));
    }

    let punct = match ch {
        b'!' => Some((49, 2)),
        b'@' => Some((50, 3)),
        b'#' => Some((51, 4)),
        b'$' => Some((52, 5)),
        b'%' => Some((53, 6)),
        b'^' => Some((54, 7)),
        b'&' => Some((55, 8)),
        b'*' => Some((56, 9)),
        b'(' => Some((57, 10)),
        b')' => Some((48, 11)),
        b'-' | b'_' => Some((189, 12)),
        b'=' | b'+' => Some((187, 13)),
        b'[' | b'{' => Some((219, 26)),
        b']' | b'}' => Some((221, 27)),
        b';' | b':' => Some((186, 39)),
        b'\'' | b'"' => Some((222, 40)),
        b'`' | b'~' => Some((192, 41)),
        b'\\' | b'|' => Some((220, 43)),
        b',' | b'<' => Some((188, 51)),
        b'.' | b'>' => Some((190, 52)),
        b'/' | b'?' => Some((191, 53)),
        _ => None,
    }?;

    Some((
        WinKeyInfo {
            vk: punct.0,
            scan: punct.1,
            enhanced: false,
            character: ch as u32,
        },
        false,
    ))
}

fn win32_control_character(cp: u32, ctrl: bool, shift: bool) -> u32 {
    if !ctrl || cp == 0 {
        return cp;
    }
    match cp {
        32 | 64 => 0,
        47 | 95 => 31,
        45 if shift => 31,
        63 => 127,
        50..=56 => [0, 27, 28, 29, 30, 31, 127][(cp - 50) as usize],
        64..=95 | 97..=122 => cp & 31,
        _ => cp,
    }
}

fn encode_navigation_key(key: &str, xterm_mod: u32, app_cursor: bool) -> Option<Vec<u8>> {
    let letter = match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(l) = letter {
        let s = match (xterm_mod, app_cursor) {
            (1, false) => format!("\x1b[{l}"),
            (1, true) => format!("\x1bO{l}"),
            _ => format!("\x1b[1;{xterm_mod}{l}"),
        };
        return Some(s.into_bytes());
    }

    let tilde_code = match key {
        "insert" => Some(2u32),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        _ => None,
    };
    if let Some(code) = tilde_code {
        let s = if xterm_mod == 1 {
            format!("\x1b[{code}~")
        } else {
            format!("\x1b[{code};{xterm_mod}~")
        };
        return Some(s.into_bytes());
    }
    None
}

fn encode_function_key(key: &str, xterm_mod: u32) -> Option<Vec<u8>> {
    let ss3 = match key {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    };
    if let Some(l) = ss3 {
        let s = if xterm_mod == 1 {
            format!("\x1bO{l}")
        } else {
            format!("\x1b[1;{xterm_mod}{l}")
        };
        return Some(s.into_bytes());
    }

    let tilde = match key {
        "f5" => Some(15u32),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };
    if let Some(n) = tilde {
        let s = if xterm_mod == 1 {
            format!("\x1b[{n}~")
        } else {
            format!("\x1b[{n};{xterm_mod}~")
        };
        return Some(s.into_bytes());
    }
    None
}

fn encode_ctrl_char(key: &str, shift: bool) -> Option<u8> {
    if key.len() != 1 {
        return None;
    }
    let ch = key.as_bytes()[0];
    match ch {
        b'a'..=b'z' => Some(ch - b'a' + 1),
        b'A'..=b'Z' => Some(ch - b'A' + 1),
        b'@' | b'2' => Some(0),
        b'[' | b'3' => Some(27),
        b'\\' | b'4' => Some(28),
        b']' | b'5' => Some(29),
        b'^' | b'6' => Some(30),
        b'_' | b'/' | b'7' => Some(31),
        b'?' | b'8' => Some(127),
        b'-' if shift => Some(31),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::ClientTermState;

    fn base_state() -> ClientTermState {
        ClientTermState {
            cols: 80,
            rows: 24,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            application_cursor: false,
            bracketed_paste: false,
            modify_other_keys: 0,
            kitty_keyboard_flags: 0,
            scroll_offset: 0,
            max_scroll_offset: 0,
            mouse_tracking: false,
        }
    }

    #[test]
    fn kitty_disambiguate_and_event_types_encode_csi_u() {
        let mut st = base_state();
        st.kitty_keyboard_flags = 1; // DISAMBIGUATE_ESC_CODES
        // Shift+Enter -> CSI 13;2u
        let shift_enter =
            KeyInput::simple("enter", Some(13), true, false, false, KeyEventKind::Press);
        assert_eq!(encode_key(&shift_enter, &st, "linux", false), b"\x1b[13;2u");
        // Plain Enter in flag=1 stays CR
        let plain_enter =
            KeyInput::simple("enter", Some(13), false, false, false, KeyEventKind::Press);
        assert_eq!(encode_key(&plain_enter, &st, "linux", false), b"\r");
        // Escape in flag=1 -> CSI 27u
        let esc = KeyInput::simple("escape", Some(27), false, false, false, KeyEventKind::Press);
        assert_eq!(encode_key(&esc, &st, "linux", false), b"\x1b[27u");
        // Ctrl+J in flag=1 -> CSI 106;5u
        let ctrl_j = KeyInput::simple(
            "j",
            Some(b'j' as u32),
            false,
            false,
            true,
            KeyEventKind::Press,
        );
        assert_eq!(encode_key(&ctrl_j, &st, "linux", false), b"\x1b[106;5u");
        // Release ignored when REPORT_EVENT_TYPES (2) is off
        let rel = KeyInput::simple(
            "j",
            Some(b'j' as u32),
            false,
            false,
            true,
            KeyEventKind::Release,
        );
        assert!(encode_key(&rel, &st, "linux", false).is_empty());

        // Enable REPORT_EVENT_TYPES (1 | 2 = 3)
        st.kitty_keyboard_flags = 3;
        assert_eq!(encode_key(&rel, &st, "linux", false), b"\x1b[106;5:3u");
        let rep = KeyInput::simple("up", None, false, false, false, KeyEventKind::Repeat);
        assert_eq!(encode_key(&rep, &st, "linux", false), b"\x1b[1;1:2A");
    }

    #[test]
    fn windows_win32_input_mode_overrides_kitty_keyboard_flags() {
        let mut st = base_state();
        st.kitty_keyboard_flags = 31;
        let shift_enter =
            KeyInput::simple("enter", Some(13), true, false, false, KeyEventKind::Press);
        assert_eq!(
            encode_key(&shift_enter, &st, "windows", true),
            b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_"
        );
    }

    #[test]
    fn four_tier_priority_order_and_alternate_keys() {
        let mut st = base_state();
        let shift_enter =
            KeyInput::simple("enter", Some(13), true, false, false, KeyEventKind::Press);

        // Tier 4: Legacy VT
        assert_eq!(encode_key(&shift_enter, &st, "linux", false), b"\r");

        // Tier 3: modifyOtherKeys
        st.modify_other_keys = 2;
        assert_eq!(
            encode_key(&shift_enter, &st, "linux", false),
            b"\x1b[27;2;13~"
        );

        // Tier 2: Kitty CSI-u overrides modifyOtherKeys
        st.kitty_keyboard_flags = 1;
        assert_eq!(encode_key(&shift_enter, &st, "linux", false), b"\x1b[13;2u");

        // Tier 1: win32-input-mode overrides Kitty CSI-u and modifyOtherKeys
        assert_eq!(
            encode_key(&shift_enter, &st, "windows", true),
            b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_"
        );

        // REPORT_ALTERNATE_KEYS (4) | REPORT_ALL_KEYS_AS_ESC (8) | REPORT_ASSOCIATED_TEXT (16)
        st.kitty_keyboard_flags = 1 | 4 | 8 | 16;
        let shift_a = KeyInput {
            key: "a".into(),
            codepoint: Some(97),
            shifted_codepoint: Some(65),
            base_layout_codepoint: None,
            shift: true,
            alt: false,
            ctrl: false,
            super_key: false,
            kind: KeyEventKind::Press,
        };
        assert_eq!(
            encode_key(&shift_a, &st, "linux", false),
            b"\x1b[97:65;2;65u"
        );
    }
}
