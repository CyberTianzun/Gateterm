import {
  type Block,
  type Message,
  type Role,
  filterUserPromptMessages,
  userPromptTextBlocks,
} from "./chat";
import { writeClipboard } from "./clipboard";
import { t } from "./i18n";
import { renderMarkdown } from "./markdown";

export interface RenderMessagesOptions {
  prepend?: boolean;
  header?: HTMLElement;
  userOnly?: boolean;
  onToggleUserOnly?: () => void;
  onPickUserMessage?: (messageId: string) => void;
  jumpToId?: string | null;
}

const savedExpanded = new WeakMap<HTMLElement, { scope: string; keys: Set<string> }>();

function funnelIcon(): Element {
  const ns = "http://www.w3.org/2000/svg";
  const create = typeof document.createElementNS === "function"
    ? (tag: string) => document.createElementNS(ns, tag)
    : (tag: string) => document.createElement(tag);
  const svg = create("svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.8");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  const path = create("path");
  path.setAttribute("d", "M3 5h18l-7 8v5l-4 2v-7L3 5z");
  svg.appendChild(path);
  return svg;
}

function clearFilterButton(onToggle: () => void): HTMLButtonElement {
  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "chat-filter-clear";
  clear.textContent = t("chat.filterClear");
  clear.addEventListener("click", onToggle);
  return clear;
}

function buildToolbar(userOnly: boolean, count: number, onToggle: () => void): { toolbar: HTMLElement; filterBtn: HTMLButtonElement } {
  const toolbar = document.createElement("div");
  toolbar.className = "chat-toolbar";

  const filterBtn = document.createElement("button");
  filterBtn.type = "button";
  filterBtn.className = "chat-filter-btn";
  filterBtn.setAttribute("aria-pressed", String(userOnly));
  const label = document.createElement("span");
  label.textContent = t("chat.filterUser");
  const badge = document.createElement("span");
  badge.className = "chat-filter-count";
  badge.textContent = `(${count})`;
  filterBtn.append(funnelIcon(), label, badge);
  filterBtn.addEventListener("click", onToggle);
  toolbar.appendChild(filterBtn);

  if (userOnly) {
    const hint = document.createElement("span");
    hint.className = "chat-filter-hint";
    hint.textContent = t("chat.filterBanner", { count });
    toolbar.append(hint, clearFilterButton(onToggle));
  }

  return { toolbar, filterBtn };
}

function excerpt(text: string): string {
  return text.split(/\r?\n/).find((line) => line.trim())?.trim().slice(0, 160) ?? "";
}

function blockNode(block: Block, role: Role): HTMLElement {
  if (block.kind === "text") {
    const node = document.createElement("div");
    if (role === "assistant") {
      node.className = "cb-md";
      node.appendChild(
        renderMarkdown(block.text, {
          copyLabel: t("chat.copyCode"),
          copiedLabel: t("chat.copiedCode"),
          onCopy: writeClipboard,
        }),
      );
    } else {
      node.className = "cb-text";
      node.textContent = block.text;
    }
    return node;
  }

  const details = document.createElement("details");
  const summary = document.createElement("summary");
  const label = document.createElement("span");
  label.className = "cb-label";
  const body = document.createElement(block.kind === "thinking" ? "div" : "pre");
  summary.appendChild(label);
  details.append(summary, body);

  if (block.kind === "thinking") {
    details.className = "cb-thinking";
    label.textContent = t("chat.thinking");
    body.textContent = block.text;
  } else {
    details.className = block.kind === "tool-use" ? "cb-tool" : "cb-result";
    const preview = document.createElement("span");
    preview.className = "cb-preview";
    summary.appendChild(preview);
    if (block.kind === "tool-use") {
      label.textContent = block.name;
      preview.textContent = excerpt(block.input);
      body.textContent = block.input;
    } else {
      label.textContent = t("chat.toolResult");
      preview.textContent = excerpt(block.text);
      body.textContent = block.text;
      if (!block.ok) {
        details.classList.add("is-error");
        const error = document.createElement("span");
        error.className = "cb-result-tag";
        error.textContent = t("chat.toolResultError");
        summary.insertBefore(error, preview);
      }
    }
  }
  return details;
}

function buildEmptyFilterState(onToggle?: () => void): HTMLElement {
  const empty = document.createElement("div");
  empty.className = "chat-filter-empty";
  const text = document.createElement("div");
  text.textContent = t("chat.filterEmpty");
  empty.appendChild(text);
  if (onToggle) empty.appendChild(clearFilterButton(onToggle));
  return empty;
}

function buildUserPromptRow(message: Message, onPick?: (messageId: string) => void): HTMLElement {
  const row = document.createElement("div");
  row.className = `cmsg c-${message.role} is-jumpable`;
  row.dataset.messageId = message.id;
  row.setAttribute("role", "button");
  row.tabIndex = 0;
  for (const block of userPromptTextBlocks(message)) {
    row.appendChild(blockNode(block, message.role));
  }
  const cue = document.createElement("span");
  cue.className = "cmsg-jump-cue";
  cue.setAttribute("aria-hidden", "true");
  cue.textContent = "↗";
  row.appendChild(cue);
  const pick = () => onPick?.(message.id);
  row.addEventListener("click", pick);
  row.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      pick();
    }
  });
  return row;
}

/** Preserve disclosure state and the reading position across transcript updates. */
export function renderMessages(
  box: HTMLElement,
  messages: Message[],
  scope: string,
  scrollContainer: HTMLElement = box,
  options: RenderMessagesOptions = {},
): void {
  const sameScope = box.dataset.chatScope === scope;
  const wasUserOnly = box.dataset.chatUserOnly === "true";
  const userOnly = options.userOnly === true;
  const jumpId = !userOnly ? options.jumpToId ?? null : null;
  const scrollTop = sameScope ? scrollContainer.scrollTop : 0;
  // Measure before replacing children: an empty container always looks like its bottom.
  const atBottom = !sameScope || scrollContainer.scrollHeight - scrollContainer.scrollTop - scrollContainer.clientHeight < 40;
  const anchor = sameScope && options.prepend
    ? Array.from(box.querySelectorAll<HTMLElement>("[data-message-id]")).find((row) =>
      row.getBoundingClientRect().bottom >= scrollContainer.getBoundingClientRect().top)
    : undefined;
  const anchorTop = anchor?.getBoundingClientRect().top;
  const anchorId = anchor?.dataset.messageId;

  const prevSaved = sameScope ? savedExpanded.get(box) : undefined;
  const expanded = new Set<string>(prevSaved && prevSaved.scope === scope ? prevSaved.keys : []);
  let focused: string | undefined;
  if (sameScope && !wasUserOnly) {
    box.querySelectorAll<HTMLDetailsElement>("details[data-block-key]").forEach((details) => {
      const key = details.dataset.blockKey;
      if (!key) return;
      if (details.open) expanded.add(key);
      else expanded.delete(key);
      if (details.firstElementChild === document.activeElement) focused = key;
    });
  }
  savedExpanded.set(box, { scope, keys: expanded });

  const activeEl = document.activeElement as HTMLElement | null;
  const wasFilterFocused =
    sameScope &&
    activeEl !== null &&
    typeof box.contains === "function" &&
    box.contains(activeEl) &&
    (activeEl.classList.contains("chat-filter-btn") || activeEl.classList.contains("chat-filter-clear"));

  const userPrompts = options.onToggleUserOnly || userOnly ? filterUserPromptMessages(messages) : [];
  const visible = userOnly ? userPrompts : messages;

  const content = document.createDocumentFragment();
  let toolbarEl: HTMLElement | null = null;
  let filterBtnEl: HTMLButtonElement | null = null;
  if (options.onToggleUserOnly) {
    const built = buildToolbar(userOnly, userPrompts.length, options.onToggleUserOnly);
    toolbarEl = built.toolbar;
    filterBtnEl = built.filterBtn;
    content.appendChild(toolbarEl);
  }
  if (options.header) content.appendChild(options.header);

  let focusTarget: HTMLElement | null = null;
  let jumpTarget: HTMLElement | null = null;
  if (userOnly && visible.length === 0) {
    content.appendChild(buildEmptyFilterState(options.onToggleUserOnly));
  } else {
    for (const message of visible) {
      if (userOnly) {
        content.appendChild(buildUserPromptRow(message, options.onPickUserMessage));
        continue;
      }
      const row = document.createElement("div");
      row.className = `cmsg c-${message.role}`;
      row.dataset.messageId = message.id;
      message.blocks.forEach((block, index) => {
        const node = blockNode(block, message.role);
        if (node.tagName === "DETAILS") {
          const details = node as HTMLDetailsElement;
          const key = JSON.stringify([message.id, index, block.kind]);
          details.dataset.blockKey = key;
          details.open = expanded.has(key);
          if (key === focused) focusTarget = details.firstElementChild as HTMLElement;
        }
        row.appendChild(node);
      });
      if (jumpId && message.id === jumpId) jumpTarget = row;
      content.appendChild(row);
    }
  }

  box.replaceChildren(content);
  box.dataset.chatScope = scope;
  box.dataset.chatUserOnly = userOnly ? "true" : "false";
  if (focusTarget) (focusTarget as HTMLElement).focus({ preventScroll: true });
  else if (wasFilterFocused && filterBtnEl) filterBtnEl.focus({ preventScroll: true });

  if (jumpId && jumpTarget) {
    const toolbarHeight = toolbarEl ? toolbarEl.getBoundingClientRect().height : 0;
    const targetTop =
      jumpTarget.getBoundingClientRect().top -
      scrollContainer.getBoundingClientRect().top +
      scrollContainer.scrollTop;
    scrollContainer.scrollTop = Math.max(0, targetTop - toolbarHeight - 24);
    jumpTarget.classList.add("cmsg-jump-highlight");
    const targetRef = jumpTarget;
    setTimeout(() => {
      targetRef.classList.remove("cmsg-jump-highlight");
    }, 1800);
    return;
  }

  const restored = anchorId === undefined ? undefined : Array.from(box.querySelectorAll<HTMLElement>("[data-message-id]"))
    .find((row) => row.dataset.messageId === anchorId);
  if (options.prepend && sameScope) {
    scrollContainer.scrollTop = scrollTop;
    if (restored && anchorTop !== undefined) scrollContainer.scrollTop += restored.getBoundingClientRect().top - anchorTop;
  } else scrollContainer.scrollTop = atBottom ? scrollContainer.scrollHeight : scrollTop;
}
