import { renderMessages } from "./chat-view";
import { t, localeTag } from "./i18n";
import { fetchHistory, fetchHistoryMessages, historyError, historyTitle, HistoryError, openHistory, type HistoryEntry } from "./history";

export function mountHistory(trigger: HTMLElement, token: () => string, onOpen: (session: { id: number; name: string }) => void): void {
  trigger.addEventListener("click", () => {
    const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string) => {
      const node = document.createElement(tag);
      node.className = cls;
      if (text !== undefined) node.textContent = text;
      return node;
    };
    const dialog = el("dialog", "history-dialog");
    const head = el("div", "set-head");
    const title = el("h2", "set-title", t("history.title"));
    dialog.setAttribute("aria-label", title.textContent!);
    const back = el("button", "history-back", t("history.back"));
    back.hidden = true;
    const close = el("button", "set-close", "×");
    close.setAttribute("aria-label", t("history.close"));
    close.onclick = () => dialog.close();
    head.append(back, title, close);

    const browser = el("section", "history-browser");
    const toolbar = el("form", "history-toolbar");
    const search = el("input", "");
    search.type = "search";
    search.placeholder = t("history.search");
    search.setAttribute("aria-label", t("history.search"));
    const filter = el("select", "");
    filter.setAttribute("aria-label", t("history.agent"));
    filter.append(new Option(t("history.allAgents"), ""));
    const workspace = el("select", "history-workspace");
    workspace.setAttribute("aria-label", t("history.workspace"));
    workspace.append(new Option(t("history.allWorkspaces"), ""));
    const refresh = el("button", "", t("history.refresh"));
    refresh.type = "button";
    toolbar.append(search, filter, workspace, refresh);
    const status = el("div", "history-status");
    status.setAttribute("role", "status");
    const scroll = el("div", "history-scroll");
    const list = el("div", "history-list");
    const more = el("button", "history-more", t("history.more"));
    more.hidden = true;
    const footer = el("div", "history-status");
    footer.hidden = true;
    scroll.append(list, more, footer);
    browser.append(toolbar, status, scroll);
    const preview = el("section", "history-preview");
    preview.hidden = true;
    dialog.append(head, browser, preview);
    document.body.appendChild(dialog);
    dialog.showModal();

    let revision = 0;
    let previewRevision = 0;
    let next: number | null = null;
    let opening = false;
    let indexing = false;
    let returnFocus: HTMLElement | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    const seen = new Set<string>();
    const failureText = (error: unknown) => historyError(error instanceof HistoryError ? error.reason : "unreadable");
    const schedulePoll = () => {
      clearTimeout(pollTimer);
      if (indexing && preview.hidden && dialog.open) pollTimer = setTimeout(() => void load(), 500);
    };

    function resumeButton(entry: HistoryEntry, feedback: HTMLElement): HTMLButtonElement {
      const button = el("button", "", t(entry.terminal_session !== null ? "history.openTerminal" : "history.resume"));
      const available = entry.terminal_session !== null || entry.can_resume;
      button.disabled = !available;
      button.title = entry.cwd ?? t("history.unknownDirectory");
      button.onclick = async () => {
        if (opening) return;
        opening = true;
        button.disabled = true;
        feedback.textContent = t("history.opening");
        try {
          const result = await openHistory(token(), entry);
          if (!dialog.open) return;
          dialog.close();
          onOpen(result.session);
        } catch (error) { if (dialog.open) feedback.textContent = failureText(error); }
        finally { opening = false; button.disabled = !available; }
      };
      return button;
    }

    function metadata(entry: HistoryEntry): HTMLElement {
      const meta = el("div", "history-meta history-badges");
      meta.append(el("span", "history-agent", entry.agent_name));
      const date = el("time", "", new Date(entry.updated_at * 1000).toLocaleString(localeTag()));
      date.dateTime = new Date(entry.updated_at * 1000).toISOString();
      meta.append(date, el("span", "", entry.model || t("history.unknownModel")));
      return meta;
    }

    async function showPreview(entry: HistoryEntry, origin: HTMLElement) {
      clearTimeout(timer);
      clearTimeout(pollTimer);
      const requestId = ++previewRevision;
      returnFocus = origin;
      browser.hidden = true;
      preview.hidden = false;
      back.hidden = false;
      const heading = el("h3", "", historyTitle(entry));
      const summary = el("div", "history-preview-head");
      const feedback = el("div", "history-meta");
      feedback.setAttribute("role", "status");
      const identity = el("div", "history-session-id", entry.agent_session_id);
      summary.append(heading, metadata(entry), el("div", "history-meta", entry.cwd ?? t("history.unknownDirectory")), identity);
      const actions = el("div", "history-actions");
      actions.append(resumeButton(entry, feedback), feedback);
      summary.append(actions, el("div", "history-meta", t("history.recentMessages")));
      const body = el("div", "history-transcript", t("history.loading"));
      preview.replaceChildren(summary, body);
      back.focus();
      try {
        const window = await fetchHistoryMessages(token(), entry);
        if (!dialog.open || requestId !== previewRevision) return;
        body.replaceChildren();
        if (window.source === "transcript" && window.messages.length) {
          renderMessages(body, window.messages, `${entry.agent_id}:${entry.agent_session_id}`);
        } else body.textContent = t("history.noMessages");
      } catch (error) {
        if (dialog.open && requestId === previewRevision) body.textContent = failureText(error);
      }
    }
    back.onclick = () => {
      ++previewRevision;
      preview.hidden = true;
      preview.replaceChildren();
      browser.hidden = false;
      back.hidden = true;
      returnFocus?.focus({ preventScroll: true });
      schedulePoll();
    };

    function row(entry: HistoryEntry): HTMLElement {
      const item = el("article", "history-row");
      const label = el("h3", "", historyTitle(entry));
      const description = el("div", "history-description", entry.description || t("history.noMessages"));
      description.title = description.textContent!;
      const cwd = el("div", "history-meta history-cwd", entry.cwd ?? t("history.unknownDirectory"));
      const identity = el("div", "history-session-id", entry.agent_session_id);
      const actions = el("div", "history-actions");
      const view = el("button", "", t("history.preview"));
      view.onclick = () => void showPreview(entry, view);
      actions.append(view, resumeButton(entry, status));
      if (entry.terminal_session === null && entry.unavailable_reason) {
        actions.append(el("span", "history-meta", historyError(entry.unavailable_reason)));
      }
      item.append(label, description, metadata(entry), cwd, identity, actions);
      return item;
    }

    async function load(append = false, force = false) {
      clearTimeout(pollTimer);
      const requestId = ++revision;
      const offset = append ? next ?? 0 : 0;
      status.textContent = t("history.loading");
      more.disabled = true;
      try {
        const page = await fetchHistory(token(), search.value, filter.value, offset, force, workspace.value);
        if (!dialog.open || requestId !== revision) return;
        if (!append) { seen.clear(); list.replaceChildren(); scroll.scrollTop = 0; }
        const selectedAgent = filter.value;
        filter.replaceChildren(new Option(t("history.allAgents"), ""), ...page.agents.map(a => new Option(a.name, a.id)));
        filter.value = selectedAgent;
        const selectedWorkspace = workspace.value;
        const paths = page.workspaces ?? [];
        workspace.replaceChildren(new Option(t("history.allWorkspaces"), ""), ...paths.map(p => new Option(p, p)));
        if (selectedWorkspace && !paths.includes(selectedWorkspace)) workspace.add(new Option(selectedWorkspace, selectedWorkspace));
        workspace.value = selectedWorkspace;
        workspace.title = selectedWorkspace || t("history.allWorkspaces");
        for (const entry of page.sessions) {
          const identity = `${entry.agent_id}:${entry.agent_session_id}`;
          if (!seen.has(identity)) { seen.add(identity); list.appendChild(row(entry)); }
        }
        next = page.next_offset;
        more.hidden = next === null;
        indexing = page.indexing === true;
        status.textContent = indexing ? t("history.indexing") : t(page.total ? "history.count" : "history.empty", { count: page.total });
        if (page.truncated) status.textContent += ` · ${t("history.truncated")}`;
        if (page.skipped) status.textContent += ` · ${t("history.skipped", { count: page.skipped })}`;
        footer.textContent = t("history.limit", { count: page.max_sessions ?? 1000 });
        footer.hidden = !page.max_sessions || indexing || next !== null;
        schedulePoll();
      } catch (error) { if (dialog.open && requestId === revision) status.textContent = failureText(error); }
      finally { if (requestId === revision) more.disabled = false; }
    }
    const reload = () => { clearTimeout(timer); clearTimeout(pollTimer); void load(); };
    toolbar.onsubmit = (event) => { event.preventDefault(); reload(); };
    search.oninput = () => { ++revision; clearTimeout(timer); clearTimeout(pollTimer); timer = setTimeout(() => void load(), 250); };
    filter.onchange = reload;
    workspace.onchange = reload;
    refresh.onclick = () => { clearTimeout(timer); clearTimeout(pollTimer); void load(false, true); };
    more.onclick = () => void load(true);
    dialog.addEventListener("close", () => { clearTimeout(timer); clearTimeout(pollTimer); ++revision; ++previewRevision; dialog.remove(); trigger.focus(); });
    void load();
  });
}
