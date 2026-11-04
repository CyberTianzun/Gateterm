import XCTest
@testable import Gateterm

/// End-to-end against a running `gateterm-server`; skipped unless `GATETERM_TEST_URL`
/// (a full access URL with `#token=`) is set, e.g. by `scripts/test-host.sh --live`.
@MainActor
final class LiveServerTests: XCTestCase {
    func testHandshakeNewSessionAttachAndEcho() async throws {
        guard let raw = ProcessInfo.processInfo.environment["GATETERM_TEST_URL"] else { throw XCTSkip("GATETERM_TEST_URL not set") }
        let endpoint = try ConnectionUrl.parse(raw)
        let client = GateClient()
        let term = AlacrittyTerm(cols: 80, rows: 24)
        let opened = expectation(description: "hello-ok")
        let created = expectation(description: "session created")
        let output = expectation(description: "echo output")
        var sessionId: Int64?
        var outputSeen = false
        client.onOpen = { opened.fulfill() }
        client.onFatal = { XCTFail("fatal: \($0)") }
        client.onControl = { msg in
            if case let .sessionEvent(.created, session) = msg, sessionId == nil {
                sessionId = session.id
                created.fulfill()
            }
        }
        client.onChannelData = { channel, data in
            client.noteData(channel: channel, byteCount: data.count)
            term.write(data)
            let screen = (0..<term.rows).map { y in
                String(term.capture().cells[(y * term.cols)..<((y + 1) * term.cols)].map { Character(Unicode.Scalar($0.codepoint) ?? " ") })
            }.joined(separator: "\n")
            if !outputSeen && screen.contains("gateterm-ios-ok") {
                outputSeen = true
                output.fulfill()
            }
        }
        client.connect(webSocketURL: endpoint.webSocketURL, token: endpoint.token ?? "")
        await fulfillment(of: [opened], timeout: 10)
        client.send(.newSession(name: "ios-test", command: nil, cwd: nil))
        await fulfillment(of: [created], timeout: 10)
        let id = try XCTUnwrap(sessionId)
        client.attach(sessionId: id, channel: 1, cols: 80, rows: 24, colors: TermColors.fromRGB(fg: 0xCCCCCC, bg: 0x191A1B))
        try await Task.sleep(for: .milliseconds(800))
        let keyboard = TerminalKeyboard(serverOS: "")
        client.sendInput(channel: 1, bytes: keyboard.committedText("echo gateterm-ios-ok"))
        client.sendInput(channel: 1, bytes: keyboard.committedText("\n"))
        await fulfillment(of: [output], timeout: 10)

        let snapshot = await AgentSessions.fetch(ServerHTTP(origin: endpoint.httpOrigin(), token: endpoint.token ?? ""))
        XCTAssertNotNil(snapshot as AgentSnapshot?)
        let launchers = await Launchers.fetch(ServerHTTP(origin: endpoint.httpOrigin(), token: endpoint.token ?? ""))
        XCTAssertFalse(launchers.isEmpty)
        client.send(.killSession(id: id))
        try await Task.sleep(for: .milliseconds(200))
        client.close()
    }

    func testRejectedTokenIsFatal() async throws {
        guard let raw = ProcessInfo.processInfo.environment["GATETERM_TEST_URL"] else { throw XCTSkip("GATETERM_TEST_URL not set") }
        let endpoint = try ConnectionUrl.parse(raw)
        let client = GateClient()
        let fatal = expectation(description: "fatal")
        client.onFatal = { _ in fatal.fulfill() }
        client.onOpen = { XCTFail("must not authenticate") }
        client.connect(webSocketURL: endpoint.webSocketURL, token: "wrong-token")
        await fulfillment(of: [fatal], timeout: 10)
    }
}
