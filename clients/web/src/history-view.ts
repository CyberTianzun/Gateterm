import { renderMessages } from "./chat-view";
import { t, localeTag } from "./i18n";
import { fetchHistory, fetchHistoryMessages, historyError, HistoryError, openHistory, type HistoryEntry } from "./history";

export function mountHistory(trigger: HTMLElement, token: () => string, onOpen: (session: { id: number; name: string }) => void): void {
  trigger.addEventListener("click", () => {
    const dialog = document.createElement("dialog");
    dialog.className = "history-dialog";
    const head = document.createElement("div");
    head.className = "set-head";
    const title = document.createElement("h2");
    title.className = "set-title";
    title.textContent = t("history.title");
    dialog.setAttribute("aria-label", title.textContent);
    const close = document.createElement("button");
    close.textContent = "×";
    close.setAttribute("aria-label", t("history.close"));
    close.onclick = () => dialog.close();
    head.append(title, close);

    const toolbar = document.createElement("form");
    toolbar.className = "history-toolbar";
    const search = document.createElement("input");
    search.type = "search";
    search.placeholder = t("history.search");
    search.setAttribute("aria-label", t("history.search"));
    const filter = document.createElement("select");
    filter.setAttribute("aria-label", t("history.agent"));
    const all = document.createElement("option");
    all.value = "";
    all.textContent = t("history.allAgents");
    filter.appendChild(all);
    const refresh = document.createElement("button");
    refresh.type = "button";
    refresh.textContent = t("history.refresh");
    toolbar.append(search, filter, refresh);
    const status = document.createElement("div");
    status.className = "history-status";
    status.setAttribute("role", "status");
    const list = document.createElement("div");
    list.className = "history-list";
    const more = document.createElement("button");
    more.textContent = t("history.more");
    more.hidden = true;
    const footer = document.createElement("div");
    footer.className = "history-status";
    footer.hidden = true;
    const preview = document.createElement("section");
    preview.className = "history-preview";
    preview.hidden = true;
    dialog.append(head, toolbar, status, list, more, footer, preview);
    document.body.appendChild(dialog);
    dialog.showModal();

    let revision = 0;
    let previewRevision = 0;
    let next: number | null = null;
    let opening = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    const seen = new Set<string>();
    const failure = (error: unknown) => {
      status.textContent = historyError(error instanceof HistoryError ? error.reason : "unreadable");
    };
    async function showPreview(entry: HistoryEntry) {
      clearTimeout(pollTimer);
      const requestId = ++previewRevision;
      preview.hidden = false;
      preview.textContent = t("history.loading");
      preview.scrollIntoView({ block: "start" });
      try {
        const window = await fetchHistoryMessages(token(), entry);
        if (!dialog.open || requestId !== previewRevision) return;
        preview.replaceChildren();
        const heading = document.createElement("h3");
        heading.textContent = `${entry.title} · ${t("history.recentMessages")}`;
        const body = document.createElement("div");
        body.className = "history-transcript";
        preview.append(heading, body);
        if (window.source === "transcript" && window.messages.length) {
          renderMessages(body, window.messages, `${entry.agent_id}:${entry.agent_session_id}`);
        } else body.textContent = t("history.noMessages");
      } catch (error) {
        if (dialog.open && requestId === previewRevision) {
          preview.textContent = historyError(error instanceof HistoryError ? error.reason : "unreadable");
        }
      }
    }
    function row(entry: HistoryEntry): HTMLElement {
      const item = document.createElement("article");
      item.className = "history-row";
      const label = document.createElement("h3");
      label.textContent = entry.title;
      const meta = document.createElement("div");
      meta.className = "history-meta";
      meta.textContent = `${entry.agent_name} · ${new Date(entry.updated_at * 1000).toLocaleString(localeTag())}`;
      const cwd = document.createElement("div");
      cwd.className = "history-meta";
      cwd.textContent = entry.cwd ?? t("history.unknownDirectory");
      const actions = document.createElement("div");
      actions.className = "history-actions";
      const view = document.createElement("button");
      view.textContent = t("history.preview");
      view.onclick = () => void showPreview(entry);
      const open = document.createElement("button");
      open.textContent = t(entry.terminal_session !== null ? "history.openTerminal" : "history.resume");
      open.disabled = entry.terminal_session === null && !entry.can_resume;
      open.onclick = async () => {
        if (opening) return;
        opening = true;
        open.disabled = true;
        status.textContent = t("history.opening");
        try {
          const result = await openHistory(token(), entry);
          // Closing the panel does not cancel a resume already accepted by the server.
          if (!dialog.open) return;
          dialog.close();
          onOpen(result.session);
        } catch (error) { if (dialog.open) failure(error); }
        finally { opening = false; open.disabled = false; }
      };
      actions.append(view, open);
      if (entry.terminal_session === null && entry.unavailable_reason) {
        const reason = document.createElement("span");
        reason.className = "history-meta";
        reason.textContent = historyError(entry.unavailable_reason);
        actions.appendChild(reason);
      }
      item.append(label, meta, cwd, actions);
      return item;
    }
    async function load(append = false, force = false) {
      clearTimeout(pollTimer);
      const requestId = ++revision;
      const offset = append ? next ?? 0 : 0;
      if (!append) {
        seen.clear();
        list.replaceChildren();
        preview.hidden = true;
        ++previewRevision;
      }
      status.textContent = t("history.loading");
      more.hidden = true;
      footer.hidden = true;
      try {
        const page = await fetchHistory(token(), search.value, filter.value, offset, force);
        if (!dialog.open || requestId !== revision) return;
        if (filter.options.length === 1) {
          for (const agent of page.agents) {
            const option = document.createElement("option");
            option.value = agent.id;
            option.textContent = agent.name;
            filter.appendChild(option);
          }
        }
        for (const entry of page.sessions) {
          const identity = `${entry.agent_id}:${entry.agent_session_id}`;
          if (!seen.has(identity)) { seen.add(identity); list.appendChild(row(entry)); }
        }
        next = page.next_offset;
        more.hidden = next === null;
        status.textContent = page.indexing ? t("history.indexing") : t(page.total ? "history.count" : "history.empty", { count: page.total });
        if (page.truncated) status.textContent += ` · ${t("history.truncated")}`;
        if (page.skipped) status.textContent += ` · ${t("history.skipped", { count: page.skipped })}`;
        footer.textContent = t("history.limit", { count: page.max_sessions ?? 1000 });
        footer.hidden = !page.max_sessions || page.indexing === true || next !== null;
        if (page.indexing && preview.hidden) pollTimer = setTimeout(() => void load(), 500);
      } catch (error) { if (dialog.open && requestId === revision) failure(error); }
    }
    toolbar.onsubmit = (event) => { event.preventDefault(); clearTimeout(timer); void load(); };
    search.oninput = () => { ++revision; clearTimeout(timer); clearTimeout(pollTimer); timer = setTimeout(() => void load(), 250); };
    filter.onchange = () => { clearTimeout(timer); clearTimeout(pollTimer); void load(); };
    refresh.onclick = () => { clearTimeout(timer); clearTimeout(pollTimer); void load(false, true); };
    more.onclick = () => void load(true);
    dialog.addEventListener("close", () => { clearTimeout(timer); clearTimeout(pollTimer); ++revision; ++previewRevision; dialog.remove(); trigger.focus(); });
    void load();
  });
}
