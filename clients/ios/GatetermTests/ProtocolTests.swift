#if canImport(UIKit)
import SwiftUI
#endif
import XCTest
@testable import Gateterm

/// Ported from the Android unit tests so both clients keep the same wire and URL behaviour.
final class ProtocolTests: XCTestCase {
    private func json(_ msg: ClientMsg) -> String {
        String(decoding: Frame.decode(ControlJson.encode(msg))!.payload, as: UTF8.self)
    }

    private func parse(_ text: String) -> ServerMsg? { ControlJson.parse(Data(text.utf8)) }

    func testFrameRoundTrip() {
        let encoded = Frame.encode(channel: 0x01020304, payload: Data([9, 8, 7]), flags: 2)
        XCTAssertEqual([UInt8](encoded.prefix(5)), [4, 3, 2, 1, 2])
        XCTAssertEqual(Frame.decode(encoded), Frame.Decoded(channel: 0x01020304, flags: 2, payload: Data([9, 8, 7])))
        XCTAssertNil(Frame.decode(Data([1, 2, 3])))
    }

    func testHelloEscapesTokenQuotes() {
        let text = json(.hello(token: "a\"b"))
        XCTAssertTrue(text.contains("\"type\":\"hello\""))
        XCTAssertTrue(text.contains("\"token\":\"a\\\"b\""))
        XCTAssertTrue(text.contains("\"proto\":1"))
    }

    func testAttachAndResizeCarryColorsOnlyWhenGiven() {
        let colors = TermColors.fromRGB(fg: 0xD4D4D4, bg: 0x191A1B)
        XCTAssertEqual(colors, TermColors(fg: "#d4d4d4", bg: "#191a1b"))
        let attach = JSON(string: json(.attach(id: 1, channel: 1, lastSeq: nil, cols: 80, rows: 24, colors: colors)))!
        XCTAssertEqual(attach.object("colors")?.string("fg"), "#d4d4d4")
        XCTAssertTrue(attach.raw["last_seq"] is NSNull)
        XCTAssertNotNil(JSON(string: json(.resize(channel: 1, cols: 80, rows: 24, colors: colors)))?.object("colors"))
        XCTAssertNil(JSON(string: json(.resize(channel: 1, cols: 80, rows: 24, colors: nil)))?.raw["colors"])
        let newSession = JSON(string: json(.newSession(name: nil, command: ["zsh", "-l"], cwd: nil)))!
        XCTAssertTrue(newSession.raw["name"] is NSNull)
        XCTAssertEqual(newSession.array("command") as? [String], ["zsh", "-l"])
    }

    func testParseHelloOkErrorSessionsAndResized() {
        guard case let .helloOk(proto, os, cloud, win32) = parse(#"{"type":"hello-ok","proto":1,"os":"windows","cloud_server_id":"srv-1"}"#) else {
            return XCTFail()
        }
        XCTAssertEqual(proto, 1); XCTAssertEqual(os, "windows"); XCTAssertEqual(cloud, "srv-1"); XCTAssertTrue(win32)
        for (field, expected) in [("", true), (#","win32_input_mode":false"#, false), (#","win32_input_mode":true"#, true)] {
            guard case let .helloOk(_, _, _, win32) = parse(#"{"type":"hello-ok","proto":1,"os":"windows"\#(field)}"#) else { return XCTFail() }
            XCTAssertEqual(win32, expected)
        }
        guard case let .helloOk(_, linuxOs, _, linuxWin32) = parse(#"{"type":"hello-ok","proto":1,"os":"linux"}"#) else { return XCTFail() }
        XCTAssertEqual(linuxOs, "linux"); XCTAssertFalse(linuxWin32)
        guard case let .error(message, channel) = parse(#"{"type":"error","message":"bad hello"}"#) else { return XCTFail() }
        XCTAssertEqual(message, "bad hello"); XCTAssertNil(channel)
        guard case let .sessions(list) = parse(#"{"type":"sessions","sessions":[{"id":2,"name":"zsh","cols":80,"rows":24,"created_at":1,"cwd":"/Users/alice/work"}]}"#) else {
            return XCTFail()
        }
        XCTAssertEqual(list, [SessionInfo(id: 2, name: "zsh", cols: 80, rows: 24, createdAt: 1, cwd: "/Users/alice/work")])
        guard case let .resized(ch, cols, rows) = parse(#"{"type":"resized","channel":1,"cols":100,"rows":30}"#) else { return XCTFail() }
        XCTAssertEqual([Int(ch), cols, rows], [1, 100, 30])
        guard case let .clipboard(clipCh, clipText) = parse(#"{"type":"clipboard","channel":3,"text":"copied 文本"}"#) else { return XCTFail() }
        XCTAssertEqual(clipCh, 3)
        XCTAssertEqual(clipText, "copied 文本")
        guard case .unknown("clipboard") = parse(#"{"type":"clipboard","channel":3}"#) else { return XCTFail() }
        guard case .unknown("agent-event") = parse(#"{"type":"agent-event","agent_id":"codex"}"#) else { return XCTFail() }
    }

    func testDaemonStartupLink() throws {
        let parsed = try ConnectionUrl.parse("http://192.168.1.10:7683/#token=s3cret")
        XCTAssertEqual(parsed.webSocketURL, "ws://192.168.1.10:7683/ws")
        XCTAssertEqual(parsed.httpOrigin(), "http://192.168.1.10:7683")
        XCTAssertEqual(parsed.token, "s3cret")
        XCTAssertEqual(parsed.displayHost, "192.168.1.10:7683")
        XCTAssertFalse(parsed.isLoopback)
    }

    func testUrlVariants() throws {
        XCTAssertEqual(try ConnectionUrl.parse("10.0.0.2:9000", tokenOverride: "abc").webSocketURL, "ws://10.0.0.2:9000/ws")
        XCTAssertEqual(try ConnectionUrl.parse("https://term.example/#token=x").webSocketURL, "wss://term.example/ws")
        XCTAssertEqual(try ConnectionUrl.parse("ws://host.internal:7683/ws#token=t").webSocketURL, "ws://host.internal:7683/ws")
        XCTAssertEqual(try ConnectionUrl.parse("http://h:1/#token=old", tokenOverride: "new").token, "new")
        XCTAssertEqual(try ConnectionUrl.parse("http://h:1/?token=from-query").token, "from-query")
        XCTAssertTrue(try ConnectionUrl.parse("http://127.0.0.1:7683/#token=a").isLoopback)
        XCTAssertTrue(try ConnectionUrl.parse("http://localhost:7683/#token=a").isLoopback)
        XCTAssertTrue(try ConnectionUrl.parse("http://0.0.0.0:7683/#token=a").isWildcardBind)
        XCTAssertNil(try ConnectionUrl.parse("http://192.168.0.4:7683/").token)
        XCTAssertTrue(ConnectionUrl.looksLikeURL("  https://x/#token=1 "))
        XCTAssertFalse(ConnectionUrl.looksLikeURL("not a url"))
        XCTAssertThrowsError(try ConnectionUrl.parse("   "))
    }

    func testQrFillsAddressAndTokenSeparately() throws {
        let result = try ConnectionQr.parse("http://192.168.1.2:49152/#token=a%2Bb+%26%3D%23%E5%AF%86%E7%A0%81")
        XCTAssertEqual(result, ScannedConnection(url: "ws://192.168.1.2:49152/ws", token: "a+b &=#密码", name: "192.168.1.2:49152"))
        XCTAssertEqual(try ConnectionQr.parse("http://[fd00::2]:9000/#token=t").url, "ws://[fd00::2]:9000/ws")
        XCTAssertEqual(try ConnectionQr.parse("https://term.example:9443/#token=t").url, "wss://term.example:9443/ws")
        XCTAssertEqual(try ConnectionQr.parse("http://192.168.1.2:7683/").token, "")
    }

    func testQrRejectsNonConnectionCodesAndNonRoutableTargets() {
        for raw in ["not a url", "javascript:alert(1)", "file:///etc/passwd", "http://0.0.0.0/", "http://127.0.0.1/",
                    "http://127.4.3.2/", "http://localhost/", "http://[::1]/", "http://[::]/", "http://user:pass@192.168.1.2/",
                    "http://host:0/", "http://host:70000/", "http://host/#token=%ZZ", "http://host/#token=" + String(repeating: "x", count: 8192)] {
            XCTAssertThrowsError(try ConnectionQr.parse(raw), raw)
        }
    }

    func testCloudLinkPrefersMostRecentConnection() {
        let a = ServerConnection(id: "a", name: "", url: "http://a", token: "t", createdAt: 0, lastUsedAt: 5, cloudServerId: "s")
        let b = ServerConnection(id: "b", name: "", url: "http://b", token: "t", createdAt: 0, lastUsedAt: 9, cloudServerId: "s")
        XCTAssertEqual(CloudLink.find([a, b], cloudServerId: "s")?.id, "b")
        XCTAssertNil(CloudLink.record([a], connectionId: "a", cloudServerId: "s"))
        XCTAssertEqual(CloudLink.record([a], connectionId: "a", cloudServerId: "x")?.first?.cloudServerId, "x")
    }

    func testPendingOpenIsConsumedOnceForTheTargetServer() {
        var tracker = PendingOpenTracker()
        tracker.onTap(PendingOpen(connectionId: "c", session: 3, notificationTag: "tag"))
        XCTAssertNil(tracker.onSessions(selectedServer: "other", sessions: []))
        let session = SessionInfo(id: 3, name: "", cols: 80, rows: 24, createdAt: 0)
        XCTAssertEqual(tracker.onSessions(selectedServer: "c", sessions: [session]), .open(3, notificationTag: "tag"))
        XCTAssertNil(tracker.onSessions(selectedServer: "c", sessions: [session]))
        tracker.onTap(PendingOpen(connectionId: "c", session: 4, notificationTag: nil))
        XCTAssertEqual(tracker.onSessions(selectedServer: "c", sessions: [session]), .sessionGone)
        tracker.onTap(PendingOpen(connectionId: "c", session: 4, notificationTag: nil))
        tracker.onConnectStart("d")
        XCTAssertNil(tracker.current)
    }

    func testAgentPushPayload() {
        let push = AgentPush.parse(["server_id": "srv", "session": "7", "agent_id": "codex", "kind": "permission", "tool_name": "Bash"])
        XCTAssertEqual(push?.tag, "srv:7:codex")
        XCTAssertEqual(push?.kind, .permission)
        XCTAssertNil(AgentPush.parse(["server_id": "srv", "session": "x", "agent_id": "codex", "kind": "waiting"]))
    }

    func testLocaleDefaultAndSystemResolution() {
        XCTAssertEqual(LocalePref.default, .auto)
        XCTAssertEqual(resolveLocale(.en, systemTags: ["zh-CN"]), .en)
        XCTAssertEqual(resolveLocale(.zhCN, systemTags: ["en-US"]), .zhCN)
        XCTAssertEqual(resolveLocale(.auto, systemTags: ["zh-Hans-CN", "en-US"]), .zhCN)
        XCTAssertEqual(resolveLocale(.auto, systemTags: ["en-US", "zh-Hans-US"]), .en)
        XCTAssertEqual(resolveLocale(.auto, systemTags: ["zh-Hant-TW"]), .zhCN)
        XCTAssertEqual(resolveLocale(.auto, systemTags: ["en_GB", "zh_CN"]), .en)
        XCTAssertEqual(resolveLocale(.auto, systemTags: ["fr-FR", "de-DE", "zh-Hans"]), .zhCN)
        XCTAssertEqual(resolveLocale(.auto, systemTags: ["fr-FR", "de-DE"]), .en)
        XCTAssertEqual(resolveLocale(.auto, systemTags: []), .en)
    }

    func testUserPromptsFilterIdentifiesRealPromptsAndStripsNonPromptBlocks() {
        let pureUser = ChatMessage(id: "u1", role: .user, ts: nil, blocks: [.text("Fix bug")])
        XCTAssertTrue(pureUser.isUserPrompt)
        XCTAssertEqual(pureUser.userPromptTextBlocks, ["Fix bug"])

        let mixedUser = ChatMessage(
            id: "u2",
            role: .user,
            ts: nil,
            blocks: [
                .toolResult(ok: true, text: "output"),
                .text("   "),
                .text("Now add tests"),
                .toolUse(name: "Read", input: "a.swift"),
                .text("Second line"),
            ]
        )
        XCTAssertTrue(mixedUser.isUserPrompt)
        XCTAssertEqual(mixedUser.userPromptTextBlocks, ["Now add tests", "Second line"])

        let pureToolResult = ChatMessage(id: "tr1", role: .user, ts: nil, blocks: [.toolResult(ok: true, text: "ok")])
        XCTAssertFalse(pureToolResult.isUserPrompt)
        XCTAssertEqual(pureToolResult.userPromptTextBlocks, [])

        let piToolTurn = ChatMessage(
            id: "pi1",
            role: .user,
            ts: nil,
            blocks: [.toolUse(name: "bash", input: "ls"), .toolResult(ok: true, text: "file.txt")]
        )
        XCTAssertFalse(piToolTurn.isUserPrompt)
        XCTAssertEqual(piToolTurn.userPromptTextBlocks, [])

        let blankUser = ChatMessage(id: "u-blank", role: .user, ts: nil, blocks: [.text("  \n\t ")])
        XCTAssertFalse(blankUser.isUserPrompt)
        XCTAssertEqual(blankUser.userPromptTextBlocks, [])

        let assistant = ChatMessage(id: "a1", role: .assistant, ts: nil, blocks: [.text("Sure")])
        XCTAssertFalse(assistant.isUserPrompt)
        XCTAssertEqual(assistant.userPromptTextBlocks, [])

        let system = ChatMessage(id: "s1", role: .system, ts: nil, blocks: [.text("System notice")])
        XCTAssertFalse(system.isUserPrompt)
        XCTAssertEqual(system.userPromptTextBlocks, [])

        let messages = [
            pureUser,
            assistant,
            pureToolResult,
            piToolTurn,
            system,
            mixedUser,
            blankUser,
        ]
        XCTAssertEqual(Chat.userPrompts(messages).map(\.id), ["u1", "u2"])
    }

    func testUiPaletteMarkdownColorsMeetContrastAcrossBuiltinsAndFallbacks() {
        for theme in AppearanceCodec.builtins {
            let palette = theme.palette
            let headingContrast = contrast(palette.mdHeading, palette.background)
            let codeContrast = contrast(palette.mdCode, palette.background)
            XCTAssertGreaterThanOrEqual(headingContrast, 3.0, "\(theme.name) mdHeading contrast \(headingContrast)")
            XCTAssertGreaterThanOrEqual(codeContrast, 3.0, "\(theme.name) mdCode contrast \(codeContrast)")
        }

        var pale = AppearanceCodec.light2026
        pale.name = "Pale Solarized"
        pale.background = 0xFDF6E3
        pale.foreground = 0x586E75
        pale.ansi = [
            0x073642, 0xDC322F, 0x93A1A1, 0xEEE8D5,
            0x268BD2, 0xD33682, 0x93A1A1, 0xEEE8D5,
            0x002B36, 0xCB4B16, 0xEEE8D5, 0xFDF6E3,
            0x839496, 0x6C71C4, 0xEEE8D5, 0xFDF6E3,
        ]
        let palePalette = pale.palette
        XCTAssertEqual(palePalette.mdHeading, palePalette.accent)
        XCTAssertEqual(palePalette.mdCode, palePalette.accent)
        XCTAssertGreaterThanOrEqual(contrast(palePalette.mdHeading, palePalette.background), 3.0)
        XCTAssertGreaterThanOrEqual(contrast(palePalette.mdCode, palePalette.background), 3.0)

        var extreme = pale
        extreme.ansi[4] = 0xEEE8D5
        extreme.ansi[12] = 0xFDF6E3
        let extremePalette = extreme.palette
        XCTAssertEqual(extremePalette.mdHeading, extremePalette.foreground)
        XCTAssertEqual(extremePalette.mdCode, extremePalette.foreground)
        XCTAssertGreaterThanOrEqual(contrast(extremePalette.mdHeading, extremePalette.background), 3.0)
        XCTAssertGreaterThanOrEqual(contrast(extremePalette.mdCode, extremePalette.background), 3.0)

        var shortAnsi = AppearanceCodec.dark2026
        shortAnsi.ansi = []
        let fallbackPalette = shortAnsi.palette
        XCTAssertEqual(fallbackPalette.mdHeading, shortAnsi.foreground)
        XCTAssertEqual(fallbackPalette.mdCode, shortAnsi.foreground)
    }

    func testMarkdownFfiParsingAndPureHelpers() {
        let md = """
        # H1 Title
        ### H3 Title

        ```rust
        fn main() {}
        ```

        - [x] done
        - [ ] todo
          - nested

        | Left | Center | Right |
        |:-----|:------:|------:|
        | **a** | `b` | c |

        [ok](https://example.com) [xss](javascript:alert(1))
        """
        let doc = parseMarkdown(text: md)
        let rootBlocks = doc.root.compactMap { idx -> FfiMdBlock? in
            let i = Int(idx)
            return doc.blocks.indices.contains(i) ? doc.blocks[i] : nil
        }

        let headings = rootBlocks.compactMap { block -> (UInt8, [FfiMdRun])? in
            if case let .heading(level, runs) = block { return (level, runs) }
            return nil
        }
        XCTAssertEqual(headings.count, 2)
        XCTAssertEqual(headings[0].0, 1)
        XCTAssertEqual(headings[0].1.map(\.text).joined(), "H1 Title")
        XCTAssertEqual(headings[1].0, 3)

        let codeBlocks = rootBlocks.compactMap { block -> (String?, String)? in
            if case let .codeBlock(lang, code) = block { return (lang, code) }
            return nil
        }
        XCTAssertEqual(codeBlocks.count, 1)
        XCTAssertEqual(codeBlocks[0].0, "rust")
        XCTAssertEqual(codeBlocks[0].1, "fn main() {}")

        let lists = rootBlocks.compactMap { block -> (Bool, UInt64, [FfiMdListItem])? in
            if case let .listBlock(ordered, start, items) = block { return (ordered, start, items) }
            return nil
        }
        XCTAssertEqual(lists.count, 1)
        XCTAssertFalse(lists[0].0)
        XCTAssertEqual(lists[0].2.count, 2)
        XCTAssertEqual(lists[0].2[0].checked, true)
        XCTAssertEqual(lists[0].2[1].checked, false)
        XCTAssertTrue(lists[0].2[1].children.contains { idx in
            let i = Int(idx)
            guard doc.blocks.indices.contains(i) else { return false }
            if case .listBlock = doc.blocks[i] { return true }
            return false
        })

        let tables = rootBlocks.compactMap { block -> ([FfiMdAlign], [FfiMdTableCell], [[FfiMdTableCell]])? in
            if case let .table(alignments, headers, rows) = block { return (alignments, headers, rows) }
            return nil
        }
        XCTAssertEqual(tables.count, 1)
        XCTAssertEqual(tables[0].0, [.left, .center, .right])
        XCTAssertEqual(tables[0].1.count, 3)
        XCTAssertEqual(tables[0].2.count, 1)
        XCTAssertTrue(tables[0].2[0][0].runs.first?.bold == true)
        XCTAssertTrue(tables[0].2[0][1].runs.first?.code == true)

        XCTAssertEqual(sanitizeMarkdownLinkUrl("  https://example.com/a  ")?.absoluteString, "https://example.com/a")
        XCTAssertEqual(sanitizeMarkdownLinkUrl("mailto:test@example.com")?.absoluteString, "mailto:test@example.com")
        XCTAssertNil(sanitizeMarkdownLinkUrl(nil))
        XCTAssertNil(sanitizeMarkdownLinkUrl(""))
        XCTAssertNil(sanitizeMarkdownLinkUrl("https://"))
        XCTAssertNil(sanitizeMarkdownLinkUrl("javascript:alert(1)"))
        XCTAssertNil(sanitizeMarkdownLinkUrl("data:text/html,x"))
        XCTAssertNil(sanitizeMarkdownLinkUrl("file:///etc/passwd"))
        XCTAssertNil(sanitizeMarkdownLinkUrl("https://example.com/\nbad"))

        XCTAssertEqual(markdownHeadingFontSize(1), 15.5)
        XCTAssertEqual(markdownHeadingFontSize(2), 14.5)
        XCTAssertEqual(markdownHeadingFontSize(3), 13.5)
    }

    #if canImport(UIKit)
    func testMarkdownAttributedRunsStyling() {
        let palette = AppearanceCodec.dark2026.palette
        let runs = [
            FfiMdRun(text: "both", bold: true, italic: true, strikethrough: true, code: false, link: nil),
            FfiMdRun(text: " ", bold: false, italic: false, strikethrough: false, code: false, link: nil),
            FfiMdRun(text: "code()", bold: false, italic: false, strikethrough: false, code: true, link: nil),
            FfiMdRun(text: " ", bold: false, italic: false, strikethrough: false, code: false, link: nil),
            FfiMdRun(text: "safe", bold: false, italic: false, strikethrough: false, code: false, link: "https://example.com"),
            FfiMdRun(text: " ", bold: false, italic: false, strikethrough: false, code: false, link: nil),
            FfiMdRun(text: "evil", bold: false, italic: false, strikethrough: false, code: false, link: "javascript:alert(1)"),
        ]
        let attr = buildMarkdownAttributedString(runs: runs, palette: palette, baseSize: 13.5)
        XCTAssertEqual(String(attr.characters), "both code() safe evil")
        let bothRun = attr.runs.first { String(attr[$0.range].characters) == "both" }
        XCTAssertNotNil(bothRun?.attributes[AttributeScopes.SwiftUIAttributes.FontAttribute.self])
        XCTAssertEqual(bothRun?.attributes[AttributeScopes.SwiftUIAttributes.StrikethroughStyleAttribute.self], Text.LineStyle.single)
        let safeRun = attr.runs.first { String(attr[$0.range].characters) == "safe" }
        XCTAssertEqual(safeRun?.attributes[AttributeScopes.FoundationAttributes.LinkAttribute.self]?.absoluteString, "https://example.com")
        XCTAssertEqual(safeRun?.attributes[AttributeScopes.SwiftUIAttributes.ForegroundColorAttribute.self], Color(rgb: palette.accent))
        XCTAssertEqual(safeRun?.attributes[AttributeScopes.SwiftUIAttributes.UnderlineStyleAttribute.self], Text.LineStyle.single)
        let evilRun = attr.runs.first { String(attr[$0.range].characters) == "evil" }
        XCTAssertNil(evilRun?.attributes[AttributeScopes.FoundationAttributes.LinkAttribute.self])
        let codeRun = attr.runs.first { String(attr[$0.range].characters) == "code()" }
        XCTAssertEqual(codeRun?.attributes[AttributeScopes.SwiftUIAttributes.ForegroundColorAttribute.self], Color(rgb: palette.mdCode))
        XCTAssertEqual(codeRun?.attributes[AttributeScopes.SwiftUIAttributes.BackgroundColorAttribute.self], Color(rgb: palette.panel2))
    }
    #endif
}
