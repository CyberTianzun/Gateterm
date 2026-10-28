//! Keyboard encoding for Gateterm terminal panes.
//!
//! Combines `tty7`'s xterm/DECCKM key encoder with Gateterm's `modifyOtherKeys`
//! and Windows ConPTY (`win32-input-mode`) support so coding agents (Claude Code,
//! Codex, agy) receive exact Shift+Enter, Ctrl+J, Alt+Arrows, and function keys.

use gateterm_client_core::term::ClientTermState;
use gpui::{Keystroke, Modifiers};

pub fn encode_keystroke(
    ks: &Keystroke,
    term_state: &ClientTermState,
    server_os: &str,
    has_selection: bool,
    win32_input_mode: bool,
) -> Option<Vec<u8>> {
    let m = &ks.modifiers;

    // Platform (Cmd on macOS) or standard copy/paste shortcuts are handled by actions, not PTY
    if m.platform {
        return None;
    }
    if m.control && !m.alt {
        let k = ks.key.to_ascii_lowercase();
        if k == "v" && m.shift {
            return None;
        }
        if k == "c" && (m.shift || has_selection) {
            return None;
        }
    }

    let key = ks.key.as_str();
    let xterm_mod = 1 + u32::from(m.shift) + 2 * u32::from(m.alt) + 4 * u32::from(m.control);

    // Windows ConPTY special chords (Shift+Enter, Ctrl+J, Ctrl+Enter) via win32-input-mode
    if server_os.eq_ignore_ascii_case("windows") && win32_input_mode {
        if key == "enter" && m.shift && !m.control && !m.alt {
            // VK_RETURN=13, Scan=28, Char=13, Kd=1/0, SHIFT_PRESSED=0x10 (16)
            return Some(b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_".to_vec());
        }
        if key.eq_ignore_ascii_case("j") && m.control && !m.alt && !m.shift {
            // VK_J=74, Scan=36, Char=10, Kd=1/0, LEFT_CTRL_PRESSED=0x08 (8)
            return Some(b"\x1b[74;36;10;1;8;1_\x1b[74;36;10;0;8;1_".to_vec());
        }
    }

    // XTMODKEYS (`modifyOtherKeys > 0`) for modified Enter, Tab, Backspace, Escape
    if term_state.modify_other_keys > 0 && xterm_mod > 1 {
        let special_code = match key {
            "enter" => Some(13u32),
            "tab" if !(m.shift && !m.control && !m.alt) => Some(9u32),
            "backspace" => Some(127u32),
            "escape" => Some(27u32),
            _ => None,
        };
        if let Some(code) = special_code {
            return Some(format!("\x1b[27;{xterm_mod};{code}~").into_bytes());
        }
    }

    // Standard special keys
    match key {
        "enter" => {
            if m.alt && !m.control && !m.shift {
                return Some(b"\x1b\r".to_vec());
            }
            return Some(b"\r".to_vec());
        }
        "tab" => {
            if m.shift {
                return Some(b"\x1b[Z".to_vec());
            }
            return Some(b"\t".to_vec());
        }
        "backspace" => {
            if m.control {
                return Some(vec![0x08]);
            }
            if m.alt {
                return Some(vec![0x1b, 0x7f]);
            }
            return Some(vec![0x7f]);
        }
        "escape" => {
            if m.alt {
                return Some(b"\x1b\x1b".to_vec());
            }
            return Some(vec![0x1b]);
        }
        "space" => {
            if m.control {
                return Some(vec![0x00]);
            }
            if m.alt {
                return Some(b"\x1b ".to_vec());
            }
            // Plain space will arrive via InputHandler::replace_text_in_range, or if handled here:
            return None;
        }
        _ => {}
    }

    // Cursor & Navigation keys
    if let Some(seq) = encode_navigation_key(key, xterm_mod, term_state.application_cursor) {
        return Some(seq);
    }

    // Function keys F1..F12
    if let Some(seq) = encode_function_key(key, xterm_mod) {
        return Some(seq);
    }

    // Control chords (Ctrl+A .. Ctrl+Z, Ctrl+[, Ctrl+\, Ctrl+], Ctrl+^, Ctrl+_)
    if m.control {
        if let Some(ctrl_byte) = encode_ctrl_char(key, m) {
            if m.alt {
                return Some(vec![0x1b, ctrl_byte]);
            }
            return Some(vec![ctrl_byte]);
        }
    }

    // Alt + printable ASCII
    if m.alt && !m.control {
        if key.len() == 1 {
            let mut ch = key.chars().next().unwrap();
            if m.shift {
                ch = ch.to_ascii_uppercase();
            }
            let mut out = vec![0x1b];
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            return Some(out);
        }
    }

    // Unmodified printable characters are handled by GPUI's EntityInputHandler::replace_text_in_range
    // so IME composition works seamlessly across Windows, macOS, and Linux.
    None
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

fn encode_ctrl_char(key: &str, m: &Modifiers) -> Option<u8> {
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
        b'-' if m.shift => Some(31),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn dummy_state() -> ClientTermState {
        ClientTermState {
            cols: 80,
            rows: 24,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            application_cursor: false,
            bracketed_paste: false,
            modify_other_keys: 0,
            scroll_offset: 0,
            max_scroll_offset: 0,
            mouse_tracking: false,
        }
    }

    #[test]
    fn encodes_arrows_and_decckm() {
        let mut st = dummy_state();
        let ks = Keystroke {
            modifiers: Modifiers::default(),
            key: "up".into(),
            key_char: None,
        };
        assert_eq!(encode_keystroke(&ks, &st, "linux", false, false).unwrap(), b"\x1b[A");
        st.application_cursor = true;
        assert_eq!(encode_keystroke(&ks, &st, "linux", false, false).unwrap(), b"\x1bOA");
    }

    #[test]
    fn encodes_windows_shift_enter_and_modify_other_keys() {
        let mut st = dummy_state();
        let ks = Keystroke {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            key: "enter".into(),
            key_char: None,
        };
        assert_eq!(
            encode_keystroke(&ks, &st, "windows", false, true).unwrap(),
            b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_"
        );

        st.modify_other_keys = 1;
        assert_eq!(
            encode_keystroke(&ks, &st, "linux", false, false).unwrap(),
            b"\x1b[27;2;13~"
        );
    }

    #[test]
    fn windows_without_precise_input_capability_uses_vt() {
        let st = dummy_state();
        let mut ks = Keystroke {
            modifiers: Modifiers { shift: true, ..Default::default() },
            key: "enter".into(),
            key_char: None,
        };
        assert_eq!(encode_keystroke(&ks, &st, "windows", false, false).unwrap(), b"\r");
        ks.modifiers = Modifiers { control: true, ..Default::default() };
        ks.key = "j".into();
        assert_eq!(encode_keystroke(&ks, &st, "windows", false, false).unwrap(), b"\n");
    }

    #[test]
    fn encodes_ctrl_c_unless_selected() {
        let st = dummy_state();
        let ks = Keystroke {
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
            key: "c".into(),
            key_char: None,
        };
        assert_eq!(encode_keystroke(&ks, &st, "linux", false, false), Some(vec![3]));
        assert_eq!(encode_keystroke(&ks, &st, "linux", true, false), None);
    }
}
