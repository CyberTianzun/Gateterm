//! Keyboard encoding for Gateterm terminal panes.
//!
//! Aligned 1:1 with `clients/web/src/keys.ts` and `clients/android/.../TerminalKeyboard.kt`:
//! - Delegates 4-tier encoding (`win32-input-mode` -> `Kitty CSI-u` -> `modifyOtherKeys` -> `Legacy VT`)
//!   to `gateterm_client_core::keyboard::encode_key`.
//! - Conflict-free workbench & clipboard shortcut filtering (`is_workbench_or_clipboard_shortcut`).

pub use gateterm_client_core::keyboard::{
    KeyEventKind, KeyInput, WinKeyInfo, encode_key, resolve_win32_key,
};
use gateterm_client_core::term::ClientTermState;
use gpui::Keystroke;

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
    encode_keystroke_with_kind(
        ks,
        term_state,
        server_os,
        has_selection,
        win32_input_mode,
        KeyEventKind::Press,
    )
}

pub fn encode_keystroke_with_kind(
    ks: &Keystroke,
    term_state: &ClientTermState,
    server_os: &str,
    has_selection: bool,
    win32_input_mode: bool,
    kind: KeyEventKind,
) -> Option<Vec<u8>> {
    if is_workbench_or_clipboard_shortcut(ks, has_selection) {
        return None;
    }

    let m = &ks.modifiers;
    let codepoint = ks
        .key_char
        .as_deref()
        .and_then(|s| s.chars().next())
        .or_else(|| (ks.key.chars().count() == 1).then(|| ks.key.chars().next().unwrap()))
        .map(|c| c as u32);

    let input = KeyInput {
        key: ks.key.clone(),
        codepoint,
        shifted_codepoint: None,
        base_layout_codepoint: None,
        shift: m.shift,
        alt: m.alt,
        ctrl: m.control,
        super_key: m.platform,
        kind,
    };

    let bytes = encode_key(&input, term_state, server_os, win32_input_mode);
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
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
            kitty_keyboard_flags: 0,
            kitty_clipboard_paste_mode: false,
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
            // Shift+Enter (multiline in Claude Code / Codex / Antigravity CLI)
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
    fn kitty_keyboard_flags_encode_csi_u_via_keystroke() {
        let mut st = dummy_state();
        st.kitty_keyboard_flags = 1;
        assert_eq!(
            encode_keystroke(&stroke("enter", true, false, false), &st, "linux", false, false)
                .unwrap(),
            b"\x1b[13;2u"
        );
        assert_eq!(
            encode_keystroke(&stroke("escape", false, false, false), &st, "linux", false, false)
                .unwrap(),
            b"\x1b[27u"
        );
        assert_eq!(
            encode_keystroke(&stroke("j", false, true, false), &st, "linux", false, false)
                .unwrap(),
            b"\x1b[106;5u"
        );
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
