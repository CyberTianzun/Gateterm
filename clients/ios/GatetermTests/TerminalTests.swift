import XCTest
@testable import Gateterm

/// Byte-for-byte keyboard encodings shared with Android (`TerminalKeyboardTest`) and web `keys.ts`.
final class TerminalKeyboardTests: XCTestCase {
    private func text(_ data: Data?) -> String { String(decoding: data ?? Data(), as: UTF8.self) }

    func testWindowsWithoutPreciseInputSupportUsesOrdinaryVt() {
        let k = TerminalKeyboard(serverOS: "windows")
        XCTAssertEqual(text(k.keyDown(id: 1, KeyStroke(.a))), "a")
        XCTAssertEqual(text(k.keyUp(id: 1, modifiers: KeyModifiers())), "")
        XCTAssertEqual(text(k.tap(KeyStroke(.enter))), "\r")
        XCTAssertEqual(text(k.tap(KeyStroke(.up))), "\u{1b}[A")
        XCTAssertEqual(text(k.tap(KeyStroke(.c, modifiers: KeyModifiers(ctrl: true)))), "\u{03}")
        XCTAssertEqual(text(k.committedText("j", modifiers: KeyModifiers(ctrl: true))), "\n")
        XCTAssertEqual(text(k.releaseAll()), "")
    }

    func testWindowsShortcutsMatchNativeConptyProbeRecords() {
        let k = TerminalKeyboard(serverOS: "windows", win32InputMode: true)
        let cases: [(KeyStroke, String)] = [
            (KeyStroke(.enter, modifiers: KeyModifiers(shift: true)), "13;28;13;1;16;1_"),
            (KeyStroke(.j, modifiers: KeyModifiers(ctrl: true)), "74;36;10;1;8;1_"),
            (KeyStroke(.up, modifiers: KeyModifiers(alt: true)), "38;72;0;1;258;1_"),
            (KeyStroke(.tab, modifiers: KeyModifiers(ctrl: true)), "9;15;9;1;8;1_"),
            (KeyStroke(.pageUp, modifiers: KeyModifiers(shift: true)), "33;73;0;1;272;1_"),
            (KeyStroke(.f1, modifiers: KeyModifiers(shift: true, ctrl: true)), "112;59;0;1;24;1_"),
        ]
        for (index, (stroke, expected)) in cases.enumerated() {
            XCTAssertEqual(text(k.keyDown(id: index, stroke)), "\u{1b}[\(expected)")
            XCTAssertEqual(text(k.keyUp(id: index, modifiers: stroke.modifiers)), "\u{1b}[\(expected.replacingOccurrences(of: ";1;", with: ";0;"))")
        }
    }

    func testKeyUpKeepsControlCharacterAndRepeatsSendOnlyDown() {
        let k = TerminalKeyboard(serverOS: "windows", win32InputMode: true)
        XCTAssertEqual(text(k.keyDown(id: 1, KeyStroke(.j, modifiers: KeyModifiers(ctrl: true)))), "\u{1b}[74;36;10;1;8;1_")
        XCTAssertEqual(text(k.keyUp(id: 1, modifiers: KeyModifiers())), "\u{1b}[74;36;10;0;0;1_")
        XCTAssertNil(k.keyUp(id: 1, modifiers: KeyModifiers()))
        for _ in 0..<3 { XCTAssertEqual(text(k.keyDown(id: 2, KeyStroke(.a))), "\u{1b}[65;30;97;1;0;1_") }
        XCTAssertEqual(text(k.keyUp(id: 2, modifiers: KeyModifiers())), "\u{1b}[65;30;97;0;0;1_")
    }

    func testFocusLossReleasesPressedKeysInOrder() {
        let k = TerminalKeyboard(serverOS: "windows", win32InputMode: true)
        _ = k.keyDown(id: 1, KeyStroke(.altLeft, modifiers: KeyModifiers(alt: true)))
        _ = k.keyDown(id: 2, KeyStroke(.up, modifiers: KeyModifiers(alt: true)))
        XCTAssertEqual(text(k.releaseAll()), "\u{1b}[18;56;0;0;0;1_\u{1b}[38;72;0;0;256;1_")
        XCTAssertFalse(k.hasPressed(2))
        _ = k.keyDown(id: 3, KeyStroke(.controlLeft))
        k.reset()
        XCTAssertEqual(text(k.releaseAll()), "")
    }

    func testImeTextAndSoftReturn() {
        let k = TerminalKeyboard(serverOS: "windows", win32InputMode: true)
        XCTAssertEqual(text(k.tap(KeyStroke(.c, modifiers: KeyModifiers(ctrl: true)))), "\u{1b}[67;46;3;1;8;1_\u{1b}[67;46;3;0;8;1_")
        XCTAssertEqual(text(k.committedText("j")), "j")
        XCTAssertEqual(text(k.committedText("你好😀")), "你好😀")
        XCTAssertEqual(text(k.committedText("j", modifiers: KeyModifiers(ctrl: true))), "\u{1b}[74;36;10;1;8;1_\u{1b}[74;36;10;0;8;1_")
        XCTAssertEqual(text(k.committedText("\n")), "\u{1b}[13;28;13;1;0;1_\u{1b}[13;28;13;0;0;1_")
        XCTAssertEqual(text(k.committedText("\n", modifiers: KeyModifiers(shift: true))), "\u{1b}[13;28;13;1;16;1_\u{1b}[13;28;13;0;16;1_")
    }

    func testNonWindowsPreservesVtModesAndModifiedNavigation() {
        for os in ["linux", "macos", ""] {
            let k = TerminalKeyboard(serverOS: os)
            XCTAssertEqual(text(k.tap(KeyStroke(.up))), "\u{1b}[A")
            XCTAssertEqual(text(k.tap(KeyStroke(.up), applicationCursor: true)), "\u{1b}OA")
            XCTAssertEqual(text(k.tap(KeyStroke(.up, modifiers: KeyModifiers(alt: true)))), "\u{1b}[1;3A")
            XCTAssertEqual(text(k.tap(KeyStroke(.home))), "\u{1b}[H")
            XCTAssertEqual(text(k.tap(KeyStroke(.end))), "\u{1b}[F")
            XCTAssertEqual(text(k.tap(KeyStroke(.pageUp))), "\u{1b}[5~")
            XCTAssertEqual(text(k.tap(KeyStroke(.tab, modifiers: KeyModifiers(shift: true)))), "\u{1b}[Z")
            XCTAssertEqual(text(k.tap(KeyStroke(.c, modifiers: KeyModifiers(ctrl: true)))), "\u{03}")
            XCTAssertEqual(text(k.committedText("\n", modifiers: KeyModifiers(shift: true))), "\r")
        }
    }

    func testModifyOtherKeysEncodesOnlyCombinationsLegacyLoses() {
        let k = TerminalKeyboard(serverOS: "linux")
        let cases: [(TerminalKey, Int, String)] = [
            (.enter, 1, "\u{1b}[27;2;13~"), (.enter, 4, "\u{1b}[27;5;13~"), (.enter, 3, "\u{1b}[27;4;13~"),
            (.numpadEnter, 1, "\u{1b}[27;2;13~"), (.enter, 0, "\r"), (.enter, 2, "\u{1b}\r"),
            (.tab, 4, "\u{1b}[27;5;9~"), (.tab, 1, "\u{1b}[Z"), (.backspace, 1, "\u{1b}[27;2;127~"), (.backspace, 4, "\u{08}"),
            (.backspace, 2, "\u{1b}\u{7f}"), (.escape, 1, "\u{1b}[27;2;27~"), (.escape, 2, "\u{1b}\u{1b}"),
            (.up, 1, "\u{1b}[1;2A"), (.j, 4, "\n"),
        ]
        for level in 1...2 {
            for (key, mask, want) in cases {
                let m = KeyModifiers(shift: mask & 1 != 0, ctrl: mask & 4 != 0, alt: mask & 2 != 0)
                XCTAssertEqual(text(k.tap(KeyStroke(key, modifiers: m), modifyOtherKeys: level)), want, "\(level)/\(key)/\(mask)")
            }
            XCTAssertEqual(text(k.committedText("\n", modifiers: KeyModifiers(shift: true), modifyOtherKeys: level)), "\u{1b}[27;2;13~")
        }
    }

    func testRightModifiersLocksAndLayoutArePreserved() {
        let k = TerminalKeyboard(serverOS: "windows", win32InputMode: true)
        let m = KeyModifiers(ctrl: true, alt: true, leftCtrl: true, rightCtrl: true, rightAlt: true, capsLock: true, numLock: true, scrollLock: true)
        XCTAssertEqual(text(k.keyDown(id: 1, KeyStroke(.up, modifiers: m))), "\u{1b}[38;72;0;1;493;1_")
        XCTAssertEqual(text(k.keyDown(id: 2, KeyStroke(.q, codePoint: 97))), "\u{1b}[65;16;97;1;0;1_")
        XCTAssertEqual(text(k.keyDown(id: 3, KeyStroke(.e, codePoint: 0xE9))), "é")
        XCTAssertEqual(text(k.keyUp(id: 3, modifiers: KeyModifiers())), "")
    }

    func testAsciiLookupMatchesAndroidTies() {
        XCTAssertEqual(TerminalKey.fromASCII("*"), .numpadMultiply)
        XCTAssertEqual(TerminalKey.fromASCII("+"), .numpadAdd)
        XCTAssertEqual(TerminalKey.fromASCII("-"), .minus)
        XCTAssertEqual(TerminalKey.fromASCII("!"), .digit1)
        XCTAssertEqual(TerminalKey.fromASCII("Q"), .q)
        #if canImport(UIKit)
        XCTAssertEqual(HIDKeyMap.key(UIKeyboardHIDUsage(rawValue: 0x0D)!), .j)
        XCTAssertEqual(HIDKeyMap.key(.keyboardUpArrow), .up)
        XCTAssertEqual(HIDKeyMap.key(.keypadEnter), .numpadEnter)
        #endif
    }

    func testKittyKeyboardReportAllKeysAsEscEncodesCommittedSingleAscii() {
        let k = TerminalKeyboard(serverOS: "linux")
        XCTAssertEqual(text(k.committedText("a", kittyKeyboardFlags: 8)), "\u{1b}[97u")
        XCTAssertEqual(text(k.committedText("A", modifiers: KeyModifiers(shift: true), kittyKeyboardFlags: 8)), "\u{1b}[97;2u")
    }
}

#if canImport(UIKit)
final class AgentBrandIconTests: XCTestCase {
    func testAgentBrandIconPathsParseCleanlyInside24x24ViewBox() {
        let icons: [(String, String)] = [
            ("claude", CLAUDE_SVG_PATH),
            ("codex", CODEX_SVG_PATH),
            ("gemini", GEMINI_SVG_PATH),
            ("opencode", OPENCODE_SVG_PATH),
            ("copilot", COPILOT_SVG_PATH),
            ("grok", GROK_SVG_PATH),
            ("agy", AGY_SVG_PATH),
            ("pi", PI_SVG_PATH),
        ]
        for (name, pathStr) in icons {
            let path = parseSvgPath24(pathStr)
            XCTAssertFalse(path.isEmpty, "\(name) should not be empty")
            let bounds = path.boundingRect
            XCTAssertGreaterThanOrEqual(bounds.minX, -0.5, "\(name) minX=\(bounds.minX)")
            XCTAssertGreaterThanOrEqual(bounds.minY, -0.5, "\(name) minY=\(bounds.minY)")
            XCTAssertLessThanOrEqual(bounds.maxX, 24.5, "\(name) maxX=\(bounds.maxX)")
            XCTAssertLessThanOrEqual(bounds.maxY, 24.5, "\(name) maxY=\(bounds.maxY)")
        }
    }
}
#endif

final class AgentKeyboardLayoutTests: XCTestCase {
    func testAgentLayoutPinsPagingAndMultilineKeysFirst() {
        for agent in AgentKeyboardLayouts.agents.map(\.id) + ["future-agent"] {
            let order = AgentKeyboardLayouts.order(agentId: agent, profiles: [:])
            XCTAssertEqual(Array(order.prefix(4)), [.pageUp, .pageDown, .ctrlJ, .shiftEnter])
            XCTAssertEqual(Set(order), Set(KeyboardShortcut.allCases))
            XCTAssertEqual(order.count, Set(order).count)
        }
        let profiles = ["codex": AgentKeyLayout(enabled: false)]
        XCTAssertEqual(AgentKeyboardLayouts.order(agentId: nil, profiles: profiles), KeyboardShortcut.allCases)
        XCTAssertEqual(AgentKeyboardLayouts.order(agentId: "codex", profiles: profiles), KeyboardShortcut.allCases)
    }

    func testCustomLayoutsRoundTripAndInvalidEntriesAreIgnored() {
        let profiles = [
            "codex": AgentKeyLayout(multiline: .ctrlJ, priority: [.paste, .tab]),
            "pi": AgentKeyLayout(enabled: false),
        ]
        XCTAssertEqual(AgentKeyboardLayouts.decode(AgentKeyboardLayouts.encode(profiles)), profiles)
        XCTAssertEqual(AgentKeyboardLayouts.decode("broken"), [:])
        let parsed = AgentKeyboardLayouts.decode(#"{"codex":{"multiline":"unknown","priority":["tab","unknown","tab","page-up","ctrl-j"]},"pi":{"enabled":false}}"#)
        XCTAssertEqual(parsed["codex"]?.multiline, .both)
        XCTAssertEqual(parsed["codex"]?.priority, [.tab])
        XCTAssertEqual(parsed["pi"]?.enabled, false)
    }

    func testShortcutStrokesEncodeOriginalCombinations() {
        let k = TerminalKeyboard(serverOS: "windows", win32InputMode: true)
        XCTAssertEqual(String(decoding: k.tap(KeyboardShortcut.ctrlJ.stroke!), as: UTF8.self), "\u{1b}[74;36;10;1;8;1_\u{1b}[74;36;10;0;8;1_")
        XCTAssertEqual(KeyboardShortcut.f12.stroke?.key, .f12)
        XCTAssertNil(KeyboardShortcut.paste.stroke)
    }
}

final class AgentAndChatTests: XCTestCase {
    private func msg(_ id: String, _ text: String? = nil) -> ChatMessage {
        ChatMessage(id: id, role: .assistant, ts: nil, blocks: [.text(text ?? id)])
    }

    private func window(_ messages: [ChatMessage], reset: Bool) -> ChatWindow {
        .transcript(messages: messages, cursor: "x", reset: reset, more: false, olderCursor: nil, historySupported: false)
    }

    func testParsesAllBlockKindsAndSkipsUnknownOnes() {
        let win = Chat.parseWindow(Data(#"""
        {"source":"transcript","cursor":"c1","reset":false,"more":true,"messages":[
          {"id":"m1","role":"user","ts":1791957275,"blocks":[{"kind":"text","text":"hi"}]},
          {"id":"m2","role":"assistant","ts":null,"blocks":[{"kind":"thinking","text":"hmm"},{"kind":"tool-use","name":"Bash","input":"ls -la"},
            {"kind":"tool-result","ok":false,"text":"boom"},{"kind":"image","url":"x"}]},
          {"role":"user","blocks":[]}]}
        """#.utf8))
        guard case let .transcript(messages, cursor, _, more, _, history) = win else { return XCTFail() }
        XCTAssertEqual(cursor, "c1"); XCTAssertTrue(more); XCTAssertFalse(history)
        XCTAssertEqual(messages.map(\.id), ["m1", "m2"])
        XCTAssertEqual(messages[0].ts, 1791957275)
        XCTAssertEqual(messages[1].blocks, [.thinking("hmm"), .toolUse(name: "Bash", input: "ls -la"), .toolResult(ok: false, text: "boom")])
        XCTAssertEqual(Chat.parseWindow(Data(#"{"source":"terminal","reason":"unsupported-agent"}"#.utf8)), .terminal(.unsupportedAgent))
        XCTAssertEqual(Chat.parseWindow(Data(#"{"source":"terminal","reason":"new"}"#.utf8)), .terminal(.unreadable))
        XCTAssertEqual(Chat.parseWindow(Data("<html>".utf8)), .terminal(.unreadable))
        guard case let .transcript(_, _, _, _, older, supported) = Chat.parseWindow(Data(#"{"source":"transcript","messages":[],"older_cursor":null}"#.utf8)) else { return XCTFail() }
        XCTAssertNil(older); XCTAssertTrue(supported)
    }

    func testMergeDedupesResetsAndPrepends() {
        XCTAssertEqual(Chat.merge([msg("a"), msg("b")], window([msg("b"), msg("c")], reset: false)).map(\.id), ["a", "b", "c"])
        XCTAssertEqual(Chat.merge([msg("a")], window([msg("z")], reset: true)).map(\.id), ["z"])
        XCTAssertEqual(Chat.merge([msg("z")], window([msg("a"), msg("a"), msg("b")], reset: false)).map(\.id), ["z", "a", "b"])
        let older = Chat.merge([msg("c", "latest c"), msg("d")], window([msg("a"), msg("b"), msg("c"), msg("b")], reset: false), prepend: true)
        XCTAssertEqual(older.map(\.id), ["a", "b", "c", "d"])
        XCTAssertEqual(older[2].blocks, [.text("latest c")])
        XCTAssertEqual(Chat.merge([msg("a")], .terminal(.disabled)).map(\.id), ["a"])
    }

    func testAgentSnapshotsAndEvents() {
        let snapshot = AgentSessions.parseSnapshot(Data(#"""
        {"transcript_enabled":true,"sessions":[
          {"agent_id":"codex","agent_session_id":"1","session":4,"state":"working","supports_transcript":true},
          {"agent_id":"claude-code","agent_session_id":"2","session":4,"state":"waiting","pending":"q","supports_transcript":false},
          {"agent_id":"","agent_session_id":"x"}]}
        """#.utf8))
        XCTAssertTrue(snapshot.transcriptEnabled)
        var map = AgentSessions.replaceAll(snapshot.sessions)
        XCTAssertEqual(map.count, 2)
        XCTAssertEqual(AgentSessions.forSession(map, 4)?.agentId, "claude-code")
        XCTAssertEqual(AgentSessions.chatAgentFor(map, 4)?.agentId, "codex")
        XCTAssertTrue(AgentSessions.chatAvailable(map, 4, transcriptEnabled: true))
        XCTAssertFalse(AgentSessions.chatAvailable(map, 4, transcriptEnabled: false))
        let event = AgentSession(agentId: "codex", agentSessionId: "1", session: 4, state: .idle, detail: nil, pending: nil, supportsTranscript: nil)
        map = AgentSessions.applyEvent(map, event)
        XCTAssertEqual(map[event.key]?.supportsTranscript, true)
        var done = event; done.state = .done
        map = AgentSessions.applyEvent(map, done)
        XCTAssertNil(map[event.key])
        XCTAssertFalse(AgentSessions.parseSnapshot(Data("{}".utf8)).transcriptEnabled)
        XCTAssertEqual(connectedAgentCounts([(.connected, Array(map.values)), (.reconnecting, Array(map.values))]), [.waiting: 1])
    }

    func testLaunchersDropMalformedEntries() {
        let list = Launchers.sanitize(Data(#"""
        {"launchers":[{"id":"a","name":"zsh","command":["zsh","-l"]},{"id":"b","name":"bad","command":["x",1]},
          {"id":"c","name":"empty","command":[]},{"id":"d","name":"Claude","provider":"config","command":["claude"],"cwd":"/w","detail":"AI"}]}
        """#.utf8))
        XCTAssertEqual(list.map(\.id), ["a", "d"])
        XCTAssertEqual(list[0].detail, "zsh -l")
        XCTAssertEqual(list[0].provider, "unknown")
        XCTAssertEqual(list[1].cwd, "/w")
    }
}

final class AppearanceTests: XCTestCase {
    func testExportImportRoundTripAndClamping() {
        var a = AppearanceCodec.dark2026.withFontSize(17.4).withLineHeight(1.25).withLetterSpacing(0.5).withScrollback(1234).withFont(.zedMono)
        a.selection = 0x3339_94BC
        XCTAssertEqual(AppearanceCodec.importJSON(AppearanceCodec.exportJSON(a), base: AppearanceCodec.light2026), a)
        XCTAssertNil(AppearanceCodec.importJSON("nope"))
        let clamped = AppearanceCodec.importJSON(#"{"font":{"size":99,"lineHeight":0.5},"terminal":{"scrollback":-3}}"#)!
        XCTAssertEqual(clamped.fontSize, 48)
        XCTAssertEqual(clamped.lineHeight, 1)
        XCTAssertEqual(clamped.scrollback, 0)
        XCTAssertEqual(AppearanceCodec.importJSON(#"{"theme":{"name":"Tokyo Night"}}"#)?.background, 0x1A1B26)
        XCTAssertEqual(AppearanceCodec.parseCSS("#abc"), 0xFFAA_BBCC)
        XCTAssertEqual(AppearanceCodec.parseCSS("#11223344"), 0x4411_2233)
        XCTAssertNil(AppearanceCodec.parseCSS("#12345"))
    }

    func testPaletteAndIndexedColors() {
        XCTAssertTrue(AppearanceCodec.light2026.isLight)
        XCTAssertFalse(AppearanceCodec.dark2026.isLight)
        XCTAssertEqual(TermColor.indexed(16, ansi: AppearanceCodec.dark2026.ansi), 0x000000)
        XCTAssertEqual(TermColor.indexed(231, ansi: AppearanceCodec.dark2026.ansi), 0xFFFFFF)
        XCTAssertEqual(TermColor.indexed(232, ansi: AppearanceCodec.dark2026.ansi), 0x080808)
        XCTAssertEqual(mixRGB(0xFFFFFF, 0x000000, 0.5), 0x808080)
    }
}

/// The Rust `alacritty_terminal` core through UniFFI, as used by every open session.
final class AlacrittyTermTests: XCTestCase {
    private func row(_ frame: TermFrame, _ y: Int) -> String {
        String((0..<frame.cols).map { Character(Unicode.Scalar(frame.cell($0, y).codepoint) ?? " ") }).trimmingCharacters(in: .whitespaces)
    }

    func testPrintsWrapsColoursAndResets() {
        let t = AlacrittyTerm(cols: 4, rows: 3)
        t.write(Data("abcde".utf8))
        var f = t.capture()
        XCTAssertEqual(row(f, 0), "abcd")
        XCTAssertEqual(row(f, 1), "e")
        XCTAssertEqual([t.cursorX, t.cursorY], [1, 1])
        t.write(Data("\u{1b}[1;1H\u{1b}[31mX".utf8))
        f = t.capture()
        XCTAssertEqual(f.cell(0, 0).codepoint, 88)
        XCTAssertEqual(f.cell(0, 0).fgIndex, 1)
        t.reset()
        XCTAssertFalse(t.capture().hasContent())
    }

    func testModesAndPasteEncoding() {
        let t = AlacrittyTerm(cols: 20, rows: 5)
        t.write(Data("\u{1b}[?2004h\u{1b}[?1h\u{1b}[>4;2m\u{1b}[>3u\u{1b}[?5522h".utf8))
        XCTAssertTrue(t.bracketedPaste)
        XCTAssertTrue(t.applicationCursor)
        XCTAssertEqual(t.modifyOtherKeys, 2)
        XCTAssertEqual(t.kittyKeyboardFlags, 3)
        XCTAssertTrue(t.kittyClipboardPasteMode)
        XCTAssertEqual(String(decoding: t.encodePaste("j"), as: UTF8.self), "\u{1b}[200~j\u{1b}[201~")
        XCTAssertNil(t.encodePointer(col: 0, row: 0, pressed: true, moving: false))
        t.write(Data("\u{1b}[?1000h\u{1b}[?1006h".utf8))
        XCTAssertNotNil(t.encodePointer(col: 0, row: 0, pressed: true, moving: false))
        t.write(Data("\u{1b}]8;;https://example.com\u{1b}\\L\u{1b}]8;;\u{1b}\\".utf8))
        XCTAssertEqual(t.hyperlinkAt(col: 0, row: 0), "https://example.com")
    }

    func testScrollbackAndCopy() {
        let t = AlacrittyTerm(cols: 10, rows: 3, maxScrollback: 100)
        t.write(Data((1...10).map { "line\($0)" }.joined(separator: "\r\n").utf8))
        t.scroll(by: 2)
        XCTAssertEqual(t.scrollOffset, 2)
        XCTAssertEqual(row(t.capture(), 0), "line6")
        XCTAssertEqual(t.copyViewport(TermSelection(x0: 0, y0: 0, x1: 4, y1: 0)), "line6")
        t.scrollToBottom()
        XCTAssertEqual(t.scrollOffset, 0)
    }

    func testPaintGate() {
        var gate = TermPaintGate()
        gate.onSessionOpen()
        XCTAssertFalse(gate.shouldPublishWrite)
        gate.onAttached(replay: false)
        gate.onResyncBegin()
        XCTAssertFalse(gate.shouldSendResize)
        gate.onResyncEnd()
        XCTAssertTrue(gate.shouldPublishWrite && gate.shouldSendResize)
        XCTAssertTrue(significantResize(80, 24, 0, 0))
        XCTAssertFalse(significantResize(81, 24, 80, 24))
        XCTAssertTrue(significantResize(82, 24, 80, 24))
    }

    func testBlockGlyphTables() {
        XCTAssertEqual(TerminalBlockGlyphs.rectangles(0x2580), [0, 0, 8, 4])
        XCTAssertEqual(TerminalBlockGlyphs.rectangles(0x2588), [0, 0, 8, 8])
        XCTAssertEqual(TerminalBlockGlyphs.boxSegments(0x2500), [0, 7, 9, 9, 7, 7, 16, 9])
        XCTAssertNotNil(TerminalBlockGlyphs.boxSegments(0x256C))
        XCTAssertNil(TerminalBlockGlyphs.boxSegments(0x41))
    }
}
