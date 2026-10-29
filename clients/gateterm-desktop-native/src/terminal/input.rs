//! Keyboard encoding for Gateterm terminal panes.
//!
//! Aligned 1:1 with `clients/web/src/keys.ts` and `clients/android/.../TerminalKeyboard.kt`:
//! - Full Windows ConPTY `win32-input-mode` (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`) for functional keys,
//!   navigation keys (`ENHANCED_KEY = 0x100`), function keys (`F1..F12`), and modified ASCII keys
//!   (`Shift+Enter`, `Ctrl+J`, `Alt+Arrows`, `Ctrl+Tab`, `Shift+Tab`, `Ctrl+Backspace`, `Ctrl+C`, etc.).
//! - `modifyOtherKeys` (`CSI 27;mod;code~`) matching xterm `OTHER_KEYS` exemption masks (`plain`).
//! - Conflict-free workbench & clipboard shortcut filtering (`is_workbench_or_clipboard_shortcut`).

use gateterm_client_core::term::ClientTermState;
use gpui::{Keystroke, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyInfo {
    pub vk: u32,
    pub scan: u32,
    pub enhanced: bool,
    pub character: u32,
}

/// Returns `true` if `ks` is a GUI workbench or clipboard shortcut that should be handled
/// by the application/window rather than encoded and sent to the PTY.
///
/// Mirrors `browserShortcut` in `clients/web/src/keys.ts`:
/// - `Cmd+*` (`m.platform`) on macOS
/// - `Ctrl+V`, `Ctrl+Shift+V`, `Shift+Insert` (Paste)
/// - `Ctrl+Shift+C`, `Ctrl+Insert`, or `Ctrl+C` when `has_selection` is true (Copy)
/// - Registered `Ctrl+Shift+<Key>` workbench shortcuts (`T, N, W, F, D, E, Z, B, M, H`)
pub fn is_workbench_or_clipboard_shortcut(ks: &Keystroke, has_selection: bool) -> bool {
    let m = &ks.modifiers;
    if m.platform {
        return true;
    }
    let key = ks.key.to_ascii_lowercase();
    if m.control && !m.alt {
        if key == "v" || (key == "c" && (m.shift || has_selection)) || key == "insert" {
            return true;
        }
        if m.shift
            && matches!(
                key.as_str(),
                "t" | "n" | "w" | "f" | "d" | "e" | "z" | "b" | "m" | "h"
            )
        {
            return true;
        }
    }
    m.shift && !m.control && !m.alt && key == "insert"
}

pub fn encode_keystroke(
    ks: &Keystroke,
    term_state: &ClientTermState,
    server_os: &str,
    has_selection: bool,
    win32_input_mode: bool,
) -> Option<Vec<u8>> {
    if is_workbench_or_clipboard_shortcut(ks, has_selection) {
        return None;
    }

    let m = &ks.modifiers;
    let key_lower = ks.key.to_ascii_lowercase();
    let key = key_lower.as_str();

    // 1. Windows ConPTY `win32-input-mode` (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`)
    if server_os.eq_ignore_ascii_case("windows") && win32_input_mode {
        if let Some(seq) = encode_win32_keystroke(ks, key, m) {
            return Some(seq);
        }
        return None;
    }

    let mods_mask = u32::from(m.shift) | (u32::from(m.alt) << 1) | (u32::from(m.control) << 2);
    let xterm_mod = 1 + mods_mask;

    // 2. XTMODKEYS (`modifyOtherKeys > 0`) for modified Enter, Tab, Backspace, Escape
    // Matches `otherKeys` in `clients/web/src/keys.ts` and `OTHER_KEYS` in `TerminalKeyboard.kt`:
    // - Enter (13): plain = [0, 2] (Enter and Alt+Enter keep legacy)
    // - Tab (9): plain = [0, 1, 2] (Tab, Shift+Tab CSI Z, and Alt+Tab keep legacy)
    // - Backspace (127): plain = [0, 2, 4] (BS, Alt+BS, and Ctrl+BS keep legacy)
    // - Escape (27): plain = [0, 2] (ESC and Alt+ESC keep legacy)
    if term_state.modify_other_keys > 0 {
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
            return Some(format!("\x1b[27;{xterm_mod};{code}~").into_bytes());
        }
    }

    // 3. Standard special keys
    match key {
        "enter" => {
            if m.alt {
                return Some(b"\x1b\r".to_vec());
            }
            return Some(b"\r".to_vec());
        }
        "tab" => {
            if m.shift {
                return Some(b"\x1b[Z".to_vec());
            }
            if m.alt {
                return Some(b"\x1b\t".to_vec());
            }
            return Some(b"\t".to_vec());
        }
        "backspace" => {
            let b = if m.control { 0x08 } else { 0x7f };
            if m.alt {
                return Some(vec![0x1b, b]);
            }
            return Some(vec![b]);
        }
        "escape" => {
            if m.alt {
                return Some(b"\x1b\x1b".to_vec());
            }
            return Some(vec![0x1b]);
        }
        "space" | " " => {
            if m.control {
                if m.alt {
                    return Some(vec![0x1b, 0x00]);
                }
                return Some(vec![0x00]);
            }
            if m.alt {
                return Some(b"\x1b ".to_vec());
            }
            // Plain space arrives via GPUI's EntityInputHandler::replace_text_in_range
            return None;
        }
        _ => {}
    }

    // 4. Cursor & Navigation keys
    if let Some(seq) = encode_navigation_key(key, xterm_mod, term_state.application_cursor) {
        return Some(seq);
    }

    // 5. Function keys F1..F12
    if let Some(seq) = encode_function_key(key, xterm_mod) {
        return Some(seq);
    }

    // 6. Control chords (Ctrl+A .. Ctrl+Z, Ctrl+2..8, Ctrl+[, Ctrl+\, Ctrl+], Ctrl+^, Ctrl+_, Ctrl+/)
    if m.control {
        if let Some(ctrl_byte) = encode_ctrl_char(ks.key.as_str(), m) {
            if m.alt {
                return Some(vec![0x1b, ctrl_byte]);
            }
            return Some(vec![ctrl_byte]);
        }
    }

    // 7. Alt + printable ASCII
    if m.alt && !m.control && ks.key.len() == 1 {
        let mut ch = ks.key.chars().next().unwrap();
        if ch.is_ascii() {
            if m.shift {
                ch = ch.to_ascii_uppercase();
            }
            return Some(vec![0x1b, ch as u8]);
        }
    }

    // Unmodified printable characters are handled by GPUI's EntityInputHandler::replace_text_in_range
    // so IME composition works seamlessly across Windows, macOS, and Linux.
    None
}

fn encode_win32_keystroke(ks: &Keystroke, key_lower: &str, m: &Modifiers) -> Option<Vec<u8>> {
    let (info, is_special) = resolve_win32_key(ks.key.as_str(), key_lower)?;
    // Unmodified printable characters (including plain space) are committed via
    // EntityInputHandler::replace_text_in_range so IME and dead keys are never broken.
    if !is_special && !m.control && !m.alt && !(key_lower == "space" && m.shift) {
        return None;
    }

    let cp = if info.character != 0 {
        if m.shift && (b'a'..=b'z').contains(&(info.character as u8)) {
            (info.character as u8).to_ascii_uppercase() as u32
        } else {
            info.character
        }
    } else {
        0
    };

    let uc = match info.vk {
        13 => {
            if m.control {
                10
            } else {
                13
            }
        }
        9 => 9,
        8 => {
            if m.control {
                127
            } else {
                8
            }
        }
        27 => 27,
        _ => win32_control_character(cp, m.control, m.shift),
    };

    let state = (if info.enhanced { 0x100 } else { 0 })
        | (if m.shift { 0x10 } else { 0 })
        | (if m.control { 0x08 } else { 0 })
        | (if m.alt { 0x02 } else { 0 });

    let seq = format!(
        "\x1b[{};{};{};1;{};1_\x1b[{};{};{};0;{};1_",
        info.vk, info.scan, uc, state, info.vk, info.scan, uc, state
    );
    Some(seq.into_bytes())
}

pub fn resolve_win32_key(raw_key: &str, key_lower: &str) -> Option<(WinKeyInfo, bool)> {
    // 1. Named functional, navigation, and F1..F12 keys (`is_special = true`)
    let special = match key_lower {
        "escape" => Some(WinKeyInfo { vk: 27, scan: 1, enhanced: false, character: 27 }),
        "backspace" => Some(WinKeyInfo { vk: 8, scan: 14, enhanced: false, character: 8 }),
        "tab" => Some(WinKeyInfo { vk: 9, scan: 15, enhanced: false, character: 9 }),
        "enter" => Some(WinKeyInfo { vk: 13, scan: 28, enhanced: false, character: 13 }),
        "up" => Some(WinKeyInfo { vk: 38, scan: 72, enhanced: true, character: 0 }),
        "down" => Some(WinKeyInfo { vk: 40, scan: 80, enhanced: true, character: 0 }),
        "left" => Some(WinKeyInfo { vk: 37, scan: 75, enhanced: true, character: 0 }),
        "right" => Some(WinKeyInfo { vk: 39, scan: 77, enhanced: true, character: 0 }),
        "home" => Some(WinKeyInfo { vk: 36, scan: 71, enhanced: true, character: 0 }),
        "end" => Some(WinKeyInfo { vk: 35, scan: 79, enhanced: true, character: 0 }),
        "pageup" => Some(WinKeyInfo { vk: 33, scan: 73, enhanced: true, character: 0 }),
        "pagedown" => Some(WinKeyInfo { vk: 34, scan: 81, enhanced: true, character: 0 }),
        "insert" => Some(WinKeyInfo { vk: 45, scan: 82, enhanced: true, character: 0 }),
        "delete" => Some(WinKeyInfo { vk: 46, scan: 83, enhanced: true, character: 0 }),
        "f1" => Some(WinKeyInfo { vk: 112, scan: 59, enhanced: false, character: 0 }),
        "f2" => Some(WinKeyInfo { vk: 113, scan: 60, enhanced: false, character: 0 }),
        "f3" => Some(WinKeyInfo { vk: 114, scan: 61, enhanced: false, character: 0 }),
        "f4" => Some(WinKeyInfo { vk: 115, scan: 62, enhanced: false, character: 0 }),
        "f5" => Some(WinKeyInfo { vk: 116, scan: 63, enhanced: false, character: 0 }),
        "f6" => Some(WinKeyInfo { vk: 117, scan: 64, enhanced: false, character: 0 }),
        "f7" => Some(WinKeyInfo { vk: 118, scan: 65, enhanced: false, character: 0 }),
        "f8" => Some(WinKeyInfo { vk: 119, scan: 66, enhanced: false, character: 0 }),
        "f9" => Some(WinKeyInfo { vk: 120, scan: 67, enhanced: false, character: 0 }),
        "f10" => Some(WinKeyInfo { vk: 121, scan: 68, enhanced: false, character: 0 }),
        "f11" => Some(WinKeyInfo { vk: 122, scan: 87, enhanced: false, character: 0 }),
        "f12" => Some(WinKeyInfo { vk: 123, scan: 88, enhanced: false, character: 0 }),
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

    fn stroke(key: &str, shift: bool, ctrl: bool, alt: bool) -> Keystroke {
        Keystroke {
            modifiers: Modifiers {
                shift,
                control: ctrl,
                alt,
                platform: false,
                function: false,
            },
            key: key.into(),
            key_char: None,
        }
    }

    #[test]
    fn encodes_arrows_and_decckm() {
        let mut st = dummy_state();
        let ks = stroke("up", false, false, false);
        assert_eq!(
            encode_keystroke(&ks, &st, "linux", false, false).unwrap(),
            b"\x1b[A"
        );
        st.application_cursor = true;
        assert_eq!(
            encode_keystroke(&ks, &st, "linux", false, false).unwrap(),
            b"\x1bOA"
        );
        // Modified arrow ignores DECCKM and uses CSI 1;mod A
        let alt_up = stroke("up", false, false, true);
        assert_eq!(
            encode_keystroke(&alt_up, &st, "linux", false, false).unwrap(),
            b"\x1b[1;3A"
        );
    }

    #[test]
    fn windows_shortcuts_match_native_conpty_probe_records() {
        let st = dummy_state();
        let cases = [
            // Shift+Enter (multiline in Claude Code / Codex / Gemini CLI)
            (
                stroke("enter", true, false, false),
                "\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_",
            ),
            // Ctrl+J (multiline in Claude Code / Codex)
            (
                stroke("j", false, true, false),
                "\x1b[74;36;10;1;8;1_\x1b[74;36;10;0;8;1_",
            ),
            // Ctrl+M and Ctrl+I preserve letter VKs while emitting CR (13) and HT (9)
            (
                stroke("m", false, true, false),
                "\x1b[77;50;13;1;8;1_\x1b[77;50;13;0;8;1_",
            ),
            (
                stroke("i", false, true, false),
                "\x1b[73;23;9;1;8;1_\x1b[73;23;9;0;8;1_",
            ),
            // Alt+Up (enhanced=256 | alt=2 -> 258, never turned into Ctrl+Up by ConPTY)
            (
                stroke("up", false, false, true),
                "\x1b[38;72;0;1;258;1_\x1b[38;72;0;0;258;1_",
            ),
            // Tab and Shift+Tab (Claude Code mode switch: Normal / Plan / Auto-accept)
            (
                stroke("tab", false, false, false),
                "\x1b[9;15;9;1;0;1_\x1b[9;15;9;0;0;1_",
            ),
            (
                stroke("tab", true, false, false),
                "\x1b[9;15;9;1;16;1_\x1b[9;15;9;0;16;1_",
            ),
            // Ctrl+Tab
            (
                stroke("tab", false, true, false),
                "\x1b[9;15;9;1;8;1_\x1b[9;15;9;0;8;1_",
            ),
            // Shift+PageUp
            (
                stroke("pageup", true, false, false),
                "\x1b[33;73;0;1;272;1_\x1b[33;73;0;0;272;1_",
            ),
            // Ctrl+Shift+F1
            (
                stroke("f1", true, true, false),
                "\x1b[112;59;0;1;24;1_\x1b[112;59;0;0;24;1_",
            ),
            // Ctrl+Backspace -> char=127, state=8
            (
                stroke("backspace", false, true, false),
                "\x1b[8;14;127;1;8;1_\x1b[8;14;127;0;8;1_",
            ),
        ];

        for (ks, expected) in cases {
            let actual = String::from_utf8(
                encode_keystroke(&ks, &st, "windows", false, true)
                    .unwrap_or_else(|| panic!("expected win32 record for {:?}", ks)),
            )
            .unwrap();
            assert_eq!(actual, expected, "key={:?}", ks);
        }
    }

    #[test]
    fn windows_modifier_combinations_keep_functional_key_identity() {
        let st = dummy_state();
        let keys = [
            ("enter", 13u32, 28u32, false),
            ("tab", 9, 15, false),
            ("backspace", 8, 14, false),
            ("escape", 27, 1, false),
            ("up", 38, 72, true),
            ("left", 37, 75, true),
            ("home", 36, 71, true),
            ("end", 35, 79, true),
            ("pageup", 33, 73, true),
            ("pagedown", 34, 81, true),
            ("delete", 46, 83, true),
            ("f1", 112, 59, false),
            ("f12", 123, 88, false),
        ];

        for (name, vk, scan, enhanced) in keys {
            for mask in 0..8u8 {
                let shift = (mask & 1) != 0;
                let ctrl = (mask & 2) != 0;
                let alt = (mask & 4) != 0;
                let ks = stroke(name, shift, ctrl, alt);
                let out = String::from_utf8(
                    encode_keystroke(&ks, &st, "windows", false, true).unwrap(),
                )
                .unwrap();
                let first_record = out
                    .strip_prefix("\x1b[")
                    .unwrap()
                    .split('_')
                    .next()
                    .unwrap();
                let parts: Vec<u32> = first_record
                    .split(';')
                    .map(|p| p.parse::<u32>().unwrap())
                    .collect();
                let expected_state = (if enhanced { 256 } else { 0 })
                    | (if shift { 16 } else { 0 })
                    | (if ctrl { 8 } else { 0 })
                    | (if alt { 2 } else { 0 });
                assert_eq!(parts[0], vk, "{name}/{mask} vk");
                assert_eq!(parts[1], scan, "{name}/{mask} scan");
                assert_eq!(parts[3], 1, "{name}/{mask} down");
                assert_eq!(parts[4], expected_state, "{name}/{mask} state");
            }
        }
    }

    #[test]
    fn modify_other_keys_encodes_only_combinations_legacy_loses() {
        let mut st = dummy_state();
        // [key, shift/alt/ctrl mask (shift=1, alt=2, ctrl=4), expected bytes]
        let cases: &[(&str, u8, &[u8])] = &[
            ("enter", 1, b"\x1b[27;2;13~"),
            ("enter", 4, b"\x1b[27;5;13~"),
            ("enter", 5, b"\x1b[27;6;13~"),
            ("enter", 3, b"\x1b[27;4;13~"),
            ("enter", 0, b"\r"),
            ("enter", 2, b"\x1b\r"),
            ("tab", 4, b"\x1b[27;5;9~"),
            ("tab", 5, b"\x1b[27;6;9~"),
            ("tab", 1, b"\x1b[Z"),
            ("tab", 0, b"\t"),
            ("tab", 2, b"\x1b\t"),
            ("backspace", 1, b"\x1b[27;2;127~"),
            ("backspace", 5, b"\x1b[27;6;127~"),
            ("backspace", 4, b"\x08"),
            ("backspace", 2, b"\x1b\x7f"),
            ("escape", 1, b"\x1b[27;2;27~"),
            ("escape", 4, b"\x1b[27;5;27~"),
            ("escape", 2, b"\x1b\x1b"),
            ("up", 1, b"\x1b[1;2A"),
            ("j", 4, b"\n"),
        ];

        for level in [1u8, 2u8] {
            st.modify_other_keys = level;
            for &(name, mask, want) in cases {
                let ks = stroke(name, (mask & 1) != 0, (mask & 4) != 0, (mask & 2) != 0);
                assert_eq!(
                    encode_keystroke(&ks, &st, "linux", false, false).unwrap(),
                    want,
                    "level={level} key={name} mask={mask}"
                );
            }
        }
    }

    #[test]
    fn windows_win32_input_mode_takes_precedence_over_modify_other_keys() {
        let mut st = dummy_state();
        st.modify_other_keys = 2;
        let ks = stroke("enter", true, false, false);
        assert_eq!(
            encode_keystroke(&ks, &st, "windows", false, true).unwrap(),
            b"\x1b[13;28;13;1;16;1_\x1b[13;28;13;0;16;1_"
        );
    }

    #[test]
    fn windows_without_precise_input_capability_uses_vt() {
        let st = dummy_state();
        let shift_enter = stroke("enter", true, false, false);
        assert_eq!(
            encode_keystroke(&shift_enter, &st, "windows", false, false).unwrap(),
            b"\r"
        );
        let ctrl_j = stroke("j", false, true, false);
        assert_eq!(
            encode_keystroke(&ctrl_j, &st, "windows", false, false).unwrap(),
            b"\n"
        );
    }

    #[test]
    fn agent_and_shell_chords_never_intercepted_by_workbench_shortcuts() {
        let st = dummy_state();
        // Critical agent / shell chords:
        // - Tab (completion)
        // - Shift+Tab (Claude Code mode switch)
        // - Ctrl+B (tmux prefix / Claude Code background command)
        // - Ctrl+F (readline forward-char / fish autosuggestion)
        // - Ctrl+O (Claude Code verbose transcript)
        // - Ctrl+R (Claude Code / shell history search)
        // - Ctrl+T (Claude Code task list)
        // - Ctrl+W (readline backward-kill-word)
        // - Escape (agent cancel)
        for (key, shift, ctrl, expected_unix) in [
            ("tab", false, false, &b"\t"[..]),
            ("tab", true, false, &b"\x1b[Z"[..]),
            ("b", false, true, &[2u8][..]),
            ("f", false, true, &[6u8][..]),
            ("o", false, true, &[15u8][..]),
            ("r", false, true, &[18u8][..]),
            ("t", false, true, &[20u8][..]),
            ("w", false, true, &[23u8][..]),
            ("escape", false, false, &[27u8][..]),
        ] {
            let ks = stroke(key, shift, ctrl, false);
            assert!(
                !is_workbench_or_clipboard_shortcut(&ks, false),
                "chord {:?} must not be swallowed by workbench shortcuts",
                ks
            );
            assert_eq!(
                encode_keystroke(&ks, &st, "linux", false, false).unwrap(),
                expected_unix
            );
            assert!(
                encode_keystroke(&ks, &st, "windows", false, true).is_some(),
                "chord {:?} must encode in win32-input-mode",
                ks
            );
        }
    }

    #[test]
    fn clipboard_and_workbench_shortcuts_are_filtered_from_pty() {
        let st = dummy_state();
        // Ctrl+C without selection -> SIGINT (3); with selection -> None (copied by view)
        let ctrl_c = stroke("c", false, true, false);
        assert_eq!(
            encode_keystroke(&ctrl_c, &st, "linux", false, false),
            Some(vec![3])
        );
        assert_eq!(
            encode_keystroke(&ctrl_c, &st, "windows", false, true).unwrap(),
            b"\x1b[67;46;3;1;8;1_\x1b[67;46;3;0;8;1_"
        );
        assert_eq!(encode_keystroke(&ctrl_c, &st, "linux", true, false), None);
        assert_eq!(encode_keystroke(&ctrl_c, &st, "windows", true, true), None);

        // Paste & Copy & Workbench Ctrl+Shift+* shortcuts return None
        for ks in [
            stroke("v", false, true, false),
            stroke("v", true, true, false),
            stroke("c", true, true, false),
            stroke("insert", false, true, false),
            stroke("insert", true, false, false),
            stroke("t", true, true, false),
            stroke("n", true, true, false),
            stroke("w", true, true, false),
            stroke("f", true, true, false),
            stroke("b", true, true, false),
        ] {
            assert!(is_workbench_or_clipboard_shortcut(&ks, false));
            assert_eq!(encode_keystroke(&ks, &st, "linux", false, false), None);
            assert_eq!(encode_keystroke(&ks, &st, "windows", false, true), None);
        }
    }
}
