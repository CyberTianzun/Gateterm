package ai.eclosion.gateterm.android.agent

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class ChatTest {
    private fun msg(id: String, text: String = id) =
        ChatMessage(id, ChatRole.Assistant, null, listOf(ChatBlock.Text(text)))

    @Test
    fun olderPageMetadataIsOptionalForOlderServers() {
        val old = Chat.parseWindow("""{"source":"transcript","messages":[],"cursor":"c","reset":true,"more":false}""") as ChatWindow.Transcript
        assertEquals(false, old.historySupported)
        assertEquals(null, old.olderCursor)
        val page = Chat.parseWindow("""{"source":"transcript","messages":[],"older_cursor":"opaque"}""") as ChatWindow.Transcript
        assertEquals(true, page.historySupported)
        assertEquals("opaque", page.olderCursor)
        val beginning = Chat.parseWindow("""{"source":"transcript","messages":[],"older_cursor":null}""") as ChatWindow.Transcript
        assertEquals(true, beginning.historySupported)
        assertEquals(null, beginning.olderCursor)
    }

    @Test
    fun earlierMessagesPrependAndKeepTheExistingLiveVersionAtOverlaps() {
        val win = ChatWindow.Transcript(listOf(msg("a"), msg("b"), msg("c"), msg("b")), "forward-unused", false, false)
        val result = Chat.merge(listOf(msg("c", "latest c"), msg("d")), win, prepend = true)
        assertEquals(listOf("a", "b", "c", "d"), result.map { it.id })
        assertEquals(listOf(ChatBlock.Text("latest c")), result[2].blocks)
        assertEquals(listOf("a", "b", "c"), Chat.merge(listOf(msg("old")), win.copy(reset = true), prepend = true).map { it.id })
    }

    @Test
    fun parsesAllBlockKindsAndSkipsUnknownOnes() {
        val win = Chat.parseWindow(
            """{"source":"transcript","cursor":"c1","reset":false,"more":true,"messages":[
                {"id":"m1","role":"user","ts":1791957275,"blocks":[{"kind":"text","text":"hi"}]},
                {"id":"m2","role":"assistant","ts":null,"blocks":[
                  {"kind":"thinking","text":"hmm"},
                  {"kind":"tool-use","name":"Bash","input":"ls -la"},
                  {"kind":"tool-result","ok":false,"text":"boom"},
                  {"kind":"image","url":"x"}
                ]},
                {"role":"user","blocks":[]}
            ]}""",
        ) as ChatWindow.Transcript
        assertEquals("c1", win.cursor)
        assertTrue(win.more)
        assertEquals(listOf("m1", "m2"), win.messages.map { it.id })
        assertEquals(ChatRole.User, win.messages[0].role)
        assertEquals(1791957275L, win.messages[0].ts)
        assertEquals(
            listOf(
                ChatBlock.Thinking("hmm"),
                ChatBlock.ToolUse("Bash", "ls -la"),
                ChatBlock.ToolResult(false, "boom"),
            ),
            win.messages[1].blocks,
        )
    }

    @Test
    fun fallbackReasonsAreTypedAndUnknownOnesReadAsUnreadable() {
        assertEquals(
            ChatWindow.Terminal(ChatFallback.UnsupportedAgent),
            Chat.parseWindow("""{"source":"terminal","reason":"unsupported-agent"}"""),
        )
        assertEquals(
            ChatWindow.Terminal(ChatFallback.Unreadable),
            Chat.parseWindow("""{"source":"terminal","reason":"something-new"}"""),
        )
        assertEquals(ChatWindow.Terminal(ChatFallback.Unreadable), Chat.parseWindow("<html>"))
    }

    @Test
    fun appendDedupesOnTheWindowBoundary() {
        val prev = listOf(msg("a"), msg("b"))
        val win = ChatWindow.Transcript(listOf(msg("b"), msg("c")), "x", reset = false, more = false)
        assertEquals(listOf("a", "b", "c"), Chat.merge(prev, win).map { it.id })
    }

    /** reset = 旧时间线作废,整段替换而不是接在后面 */
    @Test
    fun resetReplacesEverything() {
        val prev = listOf(msg("a"), msg("b"))
        val win = ChatWindow.Transcript(listOf(msg("z")), "x", reset = true, more = false)
        assertEquals(listOf("z"), Chat.merge(prev, win).map { it.id })
    }

    /** 列表按 id 作 key,重复 id 会崩 —— 同一批里、reset 时都要去重 */
    @Test
    fun duplicateIdsWithinOneWindowAreDropped() {
        val dup = ChatWindow.Transcript(listOf(msg("a"), msg("a"), msg("b")), "x", reset = true, more = false)
        assertEquals(listOf("a", "b"), Chat.merge(listOf(msg("z")), dup).map { it.id })
        val append = dup.copy(reset = false)
        assertEquals(listOf("z", "a", "b"), Chat.merge(listOf(msg("z")), append).map { it.id })
    }

    @Test
    fun fallbackWindowLeavesMessagesAlone() {
        val prev = listOf(msg("a"))
        assertEquals(prev, Chat.merge(prev, ChatWindow.Terminal(ChatFallback.Disabled)))
    }

    @Test
    fun identifiesRealUserPromptsAndExtractsOnlyNonBlankTextBlocks() {
        val pureUser = ChatMessage("u1", ChatRole.User, null, listOf(ChatBlock.Text("Fix bug")))
        assertTrue(pureUser.isUserPrompt())
        assertEquals(listOf(ChatBlock.Text("Fix bug")), pureUser.userPromptTextBlocks())

        val mixedUser = ChatMessage(
            "u2",
            ChatRole.User,
            null,
            listOf(
                ChatBlock.ToolResult(true, "output"),
                ChatBlock.Text("   "),
                ChatBlock.Text("Now add tests"),
                ChatBlock.ToolUse("Read", "a.kt"),
                ChatBlock.Text("Second line"),
            ),
        )
        assertTrue(mixedUser.isUserPrompt())
        assertEquals(
            listOf(ChatBlock.Text("Now add tests"), ChatBlock.Text("Second line")),
            mixedUser.userPromptTextBlocks(),
        )

        val pureToolResult = ChatMessage("tr1", ChatRole.User, null, listOf(ChatBlock.ToolResult(true, "ok")))
        assertFalse(pureToolResult.isUserPrompt())
        assertEquals(emptyList<ChatBlock.Text>(), pureToolResult.userPromptTextBlocks())

        val piToolTurn = ChatMessage(
            "pi1",
            ChatRole.User,
            null,
            listOf(ChatBlock.ToolUse("bash", "ls"), ChatBlock.ToolResult(true, "file.txt")),
        )
        assertFalse(piToolTurn.isUserPrompt())
        assertEquals(emptyList<ChatBlock.Text>(), piToolTurn.userPromptTextBlocks())

        val blankUser = ChatMessage("u-blank", ChatRole.User, null, listOf(ChatBlock.Text("  \n\t ")))
        assertFalse(blankUser.isUserPrompt())
        assertEquals(emptyList<ChatBlock.Text>(), blankUser.userPromptTextBlocks())

        val emptyUser = ChatMessage("u-empty", ChatRole.User, null, emptyList())
        assertFalse(emptyUser.isUserPrompt())

        val assistant = ChatMessage("a1", ChatRole.Assistant, null, listOf(ChatBlock.Text("Sure")))
        assertFalse(assistant.isUserPrompt())
        assertEquals(emptyList<ChatBlock.Text>(), assistant.userPromptTextBlocks())

        val system = ChatMessage("s1", ChatRole.System, null, listOf(ChatBlock.Text("System notice")))
        assertFalse(system.isUserPrompt())
        assertEquals(emptyList<ChatBlock.Text>(), system.userPromptTextBlocks())
    }

    @Test
    fun userPromptsPreservesChronologicalOrderAndSkipsNonPrompts() {
        val messages = listOf(
            ChatMessage("u1", ChatRole.User, null, listOf(ChatBlock.Text("First"))),
            ChatMessage("a1", ChatRole.Assistant, null, listOf(ChatBlock.ToolUse("bash", "ls"))),
            ChatMessage("tr1", ChatRole.User, null, listOf(ChatBlock.ToolResult(true, "out"))),
            ChatMessage("s1", ChatRole.System, null, listOf(ChatBlock.Text("notice"))),
            ChatMessage(
                "u2",
                ChatRole.User,
                null,
                listOf(ChatBlock.ToolResult(false, "err"), ChatBlock.Text("Follow up")),
            ),
            ChatMessage("u-blank", ChatRole.User, null, listOf(ChatBlock.Text("\n"))),
        )
        assertEquals(listOf("u1", "u2"), Chat.userPrompts(messages).map { it.id })
    }

    private fun userMsg(id: String, text: String) =
        ChatMessage(id, ChatRole.User, null, listOf(ChatBlock.Text(text)))

    private fun win(
        messages: List<ChatMessage>,
        olderCursor: String? = null,
        reset: Boolean = false,
        historySupported: Boolean = true,
    ) = ChatWindow.Transcript(
        messages = messages,
        cursor = "fwd",
        reset = reset,
        more = false,
        olderCursor = olderCursor,
        historySupported = historySupported,
    )

    @Test
    fun filtersAllEightHarnessPrefixesAndEmptyCodexMarker() {
        val harnessTexts = listOf(
            "# AGENTS.md instructions\nAlways run tests",
            "<environment_context>\n<cwd>/repo</cwd>\n</environment_context>",
            "<permissions instructions>Read only</permissions instructions>",
            "<developer_instructions>Be concise</developer_instructions>",
            "<system-reminder>Hook output</system-reminder>",
            "<local-command-caveat>Caveat</local-command-caveat>",
            "<command-name>/clear</command-name>",
            "This session is being continued from a previous conversation that ran out of context.",
            "  \n<system-reminder>with leading whitespace</system-reminder>  ",
            "   \n\t ",
            "## My request for Codex:\n   ",
            "IDE context\n## My request for Codex:\n \t ",
        )
        for (text in harnessTexts) {
            assertTrue("expected harness text: $text", isHarnessUserText(text))
            assertFalse("expected non-prompt for: $text", userMsg("h", text).isUserPrompt())
        }

        val mixed = ChatMessage(
            "u-mixed-harness",
            ChatRole.User,
            null,
            listOf(
                ChatBlock.ToolResult(true, "done"),
                ChatBlock.Text("<system-reminder>injected</system-reminder>"),
                ChatBlock.Text("Real prompt"),
            ),
        )
        assertTrue(mixed.isUserPrompt())
        assertEquals(listOf(ChatBlock.Text("Real prompt")), mixed.userPromptTextBlocks())

        val codexText = "Context\n## My request for Codex:\nFix it"
        assertFalse(isHarnessUserText(codexText))
        val codexWrapped = userMsg("u-codex", codexText)
        assertTrue(codexWrapped.isUserPrompt())
        assertEquals(listOf(ChatBlock.Text(codexText)), codexWrapped.userPromptTextBlocks())

        assertFalse(
            Chat.hasUserPrompt(
                listOf(
                    msg("a1", "hi"),
                    userMsg("u-sys", "<environment_context></environment_context>"),
                ),
            ),
        )
        assertTrue(Chat.hasUserPrompt(listOf(mixed)))
    }

    @Test
    fun filtersUnicodeWhitespaceHarnessTextAndEmptyCodexSuffix() {
        val unicodeCases = listOf(
            "\u00A0\u00A0",
            "\u00A0<system-reminder>injected</system-reminder>\u00A0",
            "\u00A0## My request for Codex:\u00A0\n\u00A0",
            "IDE context\n## My request for Codex:\u00A0",
        )
        for (text in unicodeCases) {
            assertTrue("expected Unicode harness/blank text: $text", isHarnessUserText(text))
            assertFalse("expected non-prompt for Unicode text: $text", userMsg("u-unicode", text).isUserPrompt())
            assertEquals(emptyList<ChatBlock.Text>(), userMsg("u-unicode", text).userPromptTextBlocks())
        }

        val realPromptWithNbsp = "\u00A0Fix Unicode trim\u00A0"
        assertFalse(isHarnessUserText(realPromptWithNbsp))
        val realMsg = userMsg("u-real-unicode", realPromptWithNbsp)
        assertTrue(realMsg.isUserPrompt())
        assertEquals(listOf(ChatBlock.Text(realPromptWithNbsp)), realMsg.userPromptTextBlocks())
    }

    @Test
    fun loadOlderUntilUserPromptStopsAfterOnePageOnHumanPrompt() = runBlocking {
        val u1 = userMsg("u1", "Hello")
        var messages = listOf(msg("a2"))
        val fetched = mutableListOf<String>()
        val pages = mutableListOf<List<String>>()
        val res = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { before ->
                fetched += before
                win(listOf(u1, msg("a1")), olderCursor = "c0")
            },
            getMessages = { messages },
            onPage = { w, incoming ->
                pages += incoming.map { it.id }
                messages = Chat.merge(messages, w, prepend = true)
            },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.FoundUserPrompt, 1), res)
        assertEquals(listOf("c1"), fetched)
        assertEquals(listOf(listOf("u1", "a1")), pages)
        assertEquals(listOf("u1", "a1", "a2"), messages.map { it.id })
    }

    @Test
    fun loadOlderUntilUserPromptSkipsAssistantToolHarnessAndEmptyPages() = runBlocking {
        var messages = listOf(msg("a-tail"))
        val responses = mapOf(
            "c1" to win(
                listOf(
                    ChatMessage("tr1", ChatRole.User, null, listOf(ChatBlock.ToolResult(true, "out"))),
                    userMsg("h1", "<system-reminder>skip</system-reminder>"),
                    msg("a1"),
                ),
                olderCursor = "c2",
            ),
            "c2" to win(emptyList(), olderCursor = "c3"),
            "c3" to win(listOf(userMsg("u1", "Find the bug"), msg("a0")), olderCursor = "c4"),
        )
        val fetched = mutableListOf<String>()
        val res = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { before ->
                fetched += before
                responses[before]
            },
            getMessages = { messages },
            onPage = { w, _ -> messages = Chat.merge(messages, w, prepend = true) },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.FoundUserPrompt, 3), res)
        assertEquals(listOf("c1", "c2", "c3"), fetched)
        assertEquals(listOf("u1", "a0", "tr1", "h1", "a1", "a-tail"), messages.map { it.id })
    }

    @Test
    fun loadOlderUntilUserPromptIgnoresBoundaryDuplicatePromptIds() = runBlocking {
        val u2 = userMsg("u2", "Second")
        val u1 = userMsg("u1", "First")
        var messages = listOf(u2, msg("a2"))
        val fetched = mutableListOf<String>()
        val res = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { before ->
                fetched += before
                if (before == "c1") win(listOf(msg("a1"), u2), olderCursor = "c2")
                else win(listOf(u1), olderCursor = "c3")
            },
            getMessages = { messages },
            onPage = { w, _ -> messages = Chat.merge(messages, w, prepend = true) },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.FoundUserPrompt, 2), res)
        assertEquals(listOf("c1", "c2"), fetched)
        assertEquals(
            listOf("a1"),
            Chat.newlyPrepended(listOf(u2, msg("a2")), win(listOf(msg("a1"), u2))).map { it.id },
        )
    }

    @Test
    fun loadOlderUntilUserPromptStopsAtBeginningAndPageLimit() = runBlocking {
        val beginImmediate = Chat.loadOlderUntilUserPrompt(
            initialCursor = null,
            isCancelled = { false },
            fetchPage = { throw AssertionError("should not fetch") },
            getMessages = { emptyList() },
            onPage = { _, _ -> fail("unexpected onPage") },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Beginning, 0), beginImmediate)

        var beginMsgs = listOf(msg("a2"))
        val beginAfterOne = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { win(listOf(msg("a1")), olderCursor = null) },
            getMessages = { beginMsgs },
            onPage = { w, _ -> beginMsgs = Chat.merge(beginMsgs, w, prepend = true) },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Beginning, 1), beginAfterOne)

        assertEquals(16, MAX_AUTO_OLDER_PAGES)
        var capMsgs = listOf(msg("a-tail"))
        var n = 0
        val capped = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c0",
            isCancelled = { false },
            fetchPage = {
                n++
                win(listOf(msg("a-$n")), olderCursor = "c$n")
            },
            getMessages = { capMsgs },
            onPage = { w, _ -> capMsgs = Chat.merge(capMsgs, w, prepend = true) },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.PageLimit, 16), capped)
        assertEquals(16, n)
    }

    @Test
    fun loadOlderUntilUserPromptStopsWithFailedOnErrorsAndStalledCursor() = runBlocking {
        // 1. Exception on page 2 keeps page 1
        var msgs1 = listOf(msg("a-tail"))
        var count = 0
        val thrown = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = {
                count++
                if (count == 1) win(listOf(msg("a1")), olderCursor = "c2")
                else throw RuntimeException("network down")
            },
            getMessages = { msgs1 },
            onPage = { w, _ -> msgs1 = Chat.merge(msgs1, w, prepend = true) },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Failed, 1), thrown)
        assertEquals(listOf("a1", "a-tail"), msgs1.map { it.id })

        // 2. Terminal fallback
        val terminal = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { ChatWindow.Terminal(ChatFallback.Unreadable) },
            getMessages = { emptyList() },
            onPage = { _, _ -> fail("unexpected onPage") },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Failed, 0), terminal)

        // 3. Null return (caught failure)
        val nullPage = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { null },
            getMessages = { emptyList() },
            onPage = { _, _ -> fail("unexpected onPage") },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Failed, 0), nullPage)

        // 4. Stalled cursor (!reset && olderCursor == cursor)
        var msgs2 = listOf(msg("a-tail"))
        val stalled = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { false },
            fetchPage = { before ->
                if (before == "c1") win(listOf(msg("a1")), olderCursor = "c2")
                else win(listOf(msg("a-dup")), olderCursor = "c2")
            },
            getMessages = { msgs2 },
            onPage = { w, _ -> msgs2 = Chat.merge(msgs2, w, prepend = true) },
            onReset = { fail("unexpected reset") },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Failed, 1), stalled)
        assertEquals(listOf("a1", "a-tail"), msgs2.map { it.id })
    }

    @Test
    fun loadOlderUntilUserPromptHandlesOldServerResetRealResetAndCancellation() = runBlocking {
        // 1. Old history server ignoring ?before= returns reset=true with same olderCursor
        var resets = 0
        var pages = 0
        val oldHistory = Chat.loadOlderUntilUserPrompt(
            initialCursor = "same",
            isHistoryPreview = true,
            isCancelled = { false },
            fetchPage = { win(listOf(msg("a-head")), olderCursor = "same", reset = true) },
            getMessages = { listOf(msg("a-head")) },
            onPage = { _, _ -> pages++ },
            onReset = { resets++ },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.UnsupportedOldServer, 0), oldHistory)
        assertEquals(0, resets)
        assertEquals(0, pages)

        // 2. Live chat receiving reset=true with same olderCursor fails cleanly
        val liveSame = Chat.loadOlderUntilUserPrompt(
            initialCursor = "same",
            isHistoryPreview = false,
            isCancelled = { false },
            fetchPage = { win(listOf(msg("a-head")), olderCursor = "same", reset = true) },
            getMessages = { listOf(msg("a-head")) },
            onPage = { _, _ -> fail("unexpected onPage") },
            onReset = { resets++ },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Failed, 0), liveSame)
        assertEquals(0, resets)

        // 3. Real reset with different olderCursor calls onReset and stops
        val resetIds = mutableListOf<List<String>>()
        val realReset = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isHistoryPreview = true,
            isCancelled = { false },
            fetchPage = { win(listOf(msg("fresh")), olderCursor = "c-new", reset = true) },
            getMessages = { listOf(msg("old")) },
            onPage = { _, _ -> fail("unexpected onPage") },
            onReset = { w -> resetIds += w.messages.map { it.id } },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Reset, 0), realReset)
        assertEquals(listOf(listOf("fresh")), resetIds)

        // 4. Mid-flight cancellation exits cleanly without mutating
        var cancelled = false
        var cancelPages = 0
        var cancelResets = 0
        val cancelledRes = Chat.loadOlderUntilUserPrompt(
            initialCursor = "c1",
            isCancelled = { cancelled },
            fetchPage = {
                cancelled = true
                win(listOf(msg("a1")), olderCursor = "c2")
            },
            getMessages = { listOf(msg("a2")) },
            onPage = { _, _ -> cancelPages++ },
            onReset = { cancelResets++ },
        )
        assertEquals(OlderLoadResult(OlderLoadStop.Cancelled, 0), cancelledRes)
        assertEquals(0, cancelPages)
        assertEquals(0, cancelResets)

        // 5. Coroutine CancellationException propagates instead of turning into Failed
        var rethrown = false
        try {
            Chat.loadOlderUntilUserPrompt(
                initialCursor = "c1",
                isCancelled = { false },
                fetchPage = { throw CancellationException("cancelled") },
                getMessages = { emptyList() },
                onPage = { _, _ -> },
                onReset = { },
            )
        } catch (_: CancellationException) {
            rethrown = true
        }
        assertTrue(rethrown)
    }
}
