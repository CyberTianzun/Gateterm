import { Lexer, type Token, type Tokens } from "marked";
import { writeClipboard } from "./clipboard";

const MAX_DEPTH = 16;
const MAX_INLINE_DEPTH = 32;
const ALLOWED_URL_RE = /^(https?:\/\/|mailto:)/i;
const CONTROL_CHAR_RE = /[\u0000-\u001f\u007f]/;
const COPY_FEEDBACK_MS = 1500;

export type MdAlign = "none" | "left" | "center" | "right";

export interface MdRun {
  text: string;
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  code: boolean;
  link: string | null;
}

export interface MdListItem {
  checked: boolean | null;
  children: MdBlock[];
}

export interface MdTableCell {
  runs: MdRun[];
}

export type MdBlock =
  | { kind: "heading"; level: number; runs: MdRun[] }
  | { kind: "paragraph"; runs: MdRun[] }
  | { kind: "code"; lang: string | null; code: string }
  | { kind: "blockquote"; children: MdBlock[] }
  | { kind: "list"; ordered: boolean; start: number; items: MdListItem[] }
  | { kind: "table"; alignments: MdAlign[]; headers: MdTableCell[]; rows: MdTableCell[][] }
  | { kind: "hr" };

export interface RenderMarkdownOptions {
  copyLabel?: string;
  copiedLabel?: string;
  onCopy?: (code: string) => void | boolean | Promise<void | boolean>;
}

interface InlineStyle {
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  link: string | null;
}

const PLAIN_STYLE: InlineStyle = {
  bold: false,
  italic: false,
  strikethrough: false,
  link: null,
};

/** Allow only http:, https:, and mailto: URLs without ASCII control characters. */
export function sanitizeLinkUrl(raw: string | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed || CONTROL_CHAR_RE.test(trimmed)) return null;
  const match = ALLOWED_URL_RE.exec(trimmed);
  if (!match || trimmed.length <= match[0].length) return null;
  return trimmed;
}

function pushRun(runs: MdRun[], next: MdRun): void {
  if (next.text.length === 0) return;
  const prev = runs[runs.length - 1];
  if (
    prev &&
    prev.bold === next.bold &&
    prev.italic === next.italic &&
    prev.strikethrough === next.strikethrough &&
    prev.code === next.code &&
    prev.link === next.link
  ) {
    prev.text += next.text;
  } else {
    runs.push(next);
  }
}

function extractPlainText(tokens: Token[] | undefined, fallback: string, depth = 0): string {
  if (!tokens || tokens.length === 0 || depth >= MAX_INLINE_DEPTH) return fallback;
  let out = "";
  for (const tok of tokens) {
    if (tok.type === "br") out += " ";
    else if (tok.type === "codespan" || tok.type === "escape" || tok.type === "html") out += tok.text;
    else if ("tokens" in tok && Array.isArray(tok.tokens) && tok.tokens.length > 0) {
      const innerFallback = "text" in tok && typeof tok.text === "string" ? tok.text : "";
      out += extractPlainText(tok.tokens, innerFallback, depth + 1);
    } else if ("text" in tok && typeof tok.text === "string") {
      out += tok.text;
    }
  }
  return out || fallback;
}

function collectInlineRuns(tokens: Token[], style: InlineStyle, out: MdRun[], depth = 0): void {
  if (depth >= MAX_INLINE_DEPTH) {
    for (const tok of tokens) {
      const fallback = "text" in tok && typeof tok.text === "string" ? tok.text : tok.raw;
      pushRun(out, { text: fallback, ...style, code: false });
    }
    return;
  }

  for (const tok of tokens) {
    switch (tok.type) {
      case "text": {
        const t = tok as Tokens.Text;
        if (t.tokens && t.tokens.length > 0) {
          collectInlineRuns(t.tokens, style, out, depth + 1);
        } else {
          pushRun(out, { text: t.text, ...style, code: false });
        }
        break;
      }
      case "escape":
      case "html": {
        pushRun(out, { text: (tok as Tokens.Escape | Tokens.HTML).text, ...style, code: false });
        break;
      }
      case "br": {
        pushRun(out, { text: "\n", ...style, code: false });
        break;
      }
      case "codespan": {
        pushRun(out, { text: (tok as Tokens.Codespan).text, ...style, code: true });
        break;
      }
      case "strong": {
        collectInlineRuns((tok as Tokens.Strong).tokens ?? [], { ...style, bold: true }, out, depth + 1);
        break;
      }
      case "em": {
        collectInlineRuns((tok as Tokens.Em).tokens ?? [], { ...style, italic: true }, out, depth + 1);
        break;
      }
      case "del": {
        collectInlineRuns((tok as Tokens.Del).tokens ?? [], { ...style, strikethrough: true }, out, depth + 1);
        break;
      }
      case "link": {
        const l = tok as Tokens.Link;
        const link = sanitizeLinkUrl(l.href) ?? style.link;
        if (l.tokens && l.tokens.length > 0) {
          collectInlineRuns(l.tokens, { ...style, link }, out, depth + 1);
        } else {
          pushRun(out, { text: l.text, ...style, code: false, link });
        }
        break;
      }
      case "image": {
        const img = tok as Tokens.Image;
        const alt = extractPlainText(img.tokens, img.text);
        const safeHref = sanitizeLinkUrl(img.href);
        const label = alt || safeHref || "image";
        const link = style.link ?? safeHref;
        pushRun(out, { text: `[${label}]`, ...style, code: false, link });
        break;
      }
      case "space":
      case "checkbox":
        break;
      default: {
        if ("tokens" in tok && Array.isArray(tok.tokens) && tok.tokens.length > 0) {
          collectInlineRuns(tok.tokens, style, out, depth + 1);
        } else if ("text" in tok && typeof tok.text === "string") {
          pushRun(out, { text: tok.text, ...style, code: false });
        } else if (tok.raw) {
          pushRun(out, { text: tok.raw, ...style, code: false });
        }
      }
    }
  }
}

function flattenInline(tokens: Token[] | undefined): MdRun[] {
  if (!tokens || tokens.length === 0) return [];
  const runs: MdRun[] = [];
  collectInlineRuns(tokens, PLAIN_STYLE, runs);
  return runs;
}

function mapAlign(align: "left" | "center" | "right" | null): MdAlign {
  return align === "left" || align === "center" || align === "right" ? align : "none";
}

function parseLeafBlock(tok: Token, out: MdBlock[]): boolean {
  switch (tok.type) {
    case "space":
    case "def":
    case "checkbox":
      return true;
    case "hr":
      out.push({ kind: "hr" });
      return true;
    case "heading": {
      const h = tok as Tokens.Heading;
      const level = Math.min(6, Math.max(1, h.depth));
      out.push({ kind: "heading", level, runs: flattenInline(h.tokens) });
      return true;
    }
    case "paragraph": {
      const p = tok as Tokens.Paragraph;
      out.push({ kind: "paragraph", runs: flattenInline(p.tokens) });
      return true;
    }
    case "code": {
      const c = tok as Tokens.Code;
      const lang = c.lang?.trim().split(/\s+/)[0] || null;
      out.push({ kind: "code", lang, code: c.text });
      return true;
    }
    case "html": {
      const rawText = (tok as Tokens.HTML).text.replace(/\n+$/, "");
      if (rawText.length > 0) {
        out.push({ kind: "paragraph", runs: [{ text: rawText, ...PLAIN_STYLE, code: false }] });
      }
      return true;
    }
    case "table": {
      const t = tok as Tokens.Table;
      out.push({
        kind: "table",
        alignments: t.align.map(mapAlign),
        headers: t.header.map((cell) => ({ runs: flattenInline(cell.tokens) })),
        rows: t.rows.map((row) => row.map((cell) => ({ runs: flattenInline(cell.tokens) }))),
      });
      return true;
    }
    default:
      return false;
  }
}

function isInlineToken(tok: Token): boolean {
  if (tok.type === "html") return !(tok as Tokens.HTML).block;
  return (
    tok.type === "text" ||
    tok.type === "escape" ||
    tok.type === "br" ||
    tok.type === "codespan" ||
    tok.type === "strong" ||
    tok.type === "em" ||
    tok.type === "del" ||
    tok.type === "link" ||
    tok.type === "image"
  );
}

/** Iteratively flatten containers past MAX_DEPTH so deep nesting never stack-overflows. */
function flattenDeepTokens(tokens: Token[], out: MdBlock[]): void {
  const stack: Token[] = [];
  for (let i = tokens.length - 1; i >= 0; i--) stack.push(tokens[i]);

  while (stack.length > 0) {
    const tok = stack.pop()!;
    if (tok.type === "blockquote") {
      const inner = (tok as Tokens.Blockquote).tokens ?? [];
      for (let i = inner.length - 1; i >= 0; i--) stack.push(inner[i]);
      continue;
    }
    if (tok.type === "list") {
      const items = (tok as Tokens.List).items;
      for (let i = items.length - 1; i >= 0; i--) {
        const inner = items[i].tokens;
        for (let j = inner.length - 1; j >= 0; j--) stack.push(inner[j]);
      }
      continue;
    }
    if (parseLeafBlock(tok, out)) continue;
    if (isInlineToken(tok)) {
      const runs = flattenInline([tok]);
      if (runs.length > 0) out.push({ kind: "paragraph", runs });
    }
  }
}

function parseListItem(item: Tokens.ListItem, depth: number): MdListItem {
  const children: MdBlock[] = [];
  let pendingInline: Token[] = [];

  const flushInline = () => {
    if (pendingInline.length === 0) return;
    const runs = flattenInline(pendingInline);
    pendingInline = [];
    if (runs.length > 0) children.push({ kind: "paragraph", runs });
  };

  for (const tok of item.tokens) {
    if (tok.type === "checkbox" || tok.type === "space") continue;
    if (isInlineToken(tok)) {
      if (tok.type === "text" && (tok as Tokens.Text).tokens?.length) {
        pendingInline.push(...(tok as Tokens.Text).tokens!);
      } else {
        pendingInline.push(tok);
      }
      continue;
    }
    flushInline();
    parseBlockTokens([tok], depth, children);
  }
  flushInline();

  return {
    checked: item.task ? Boolean(item.checked) : null,
    children,
  };
}

function parseBlockTokens(tokens: Token[], depth: number, out: MdBlock[]): void {
  if (depth >= MAX_DEPTH - 1) {
    flattenDeepTokens(tokens, out);
    return;
  }

  for (const tok of tokens) {
    if (parseLeafBlock(tok, out)) continue;

    if (tok.type === "blockquote") {
      const children: MdBlock[] = [];
      parseBlockTokens((tok as Tokens.Blockquote).tokens ?? [], depth + 1, children);
      out.push({ kind: "blockquote", children });
      continue;
    }

    if (tok.type === "list") {
      const list = tok as Tokens.List;
      out.push({
        kind: "list",
        ordered: list.ordered,
        start: typeof list.start === "number" ? list.start : 1,
        items: list.items.map((item) => parseListItem(item, depth + 1)),
      });
      continue;
    }

    if (isInlineToken(tok)) {
      const runs = flattenInline([tok]);
      if (runs.length > 0) out.push({ kind: "paragraph", runs });
    }
  }
}

/** Parse CommonMark + GFM text into a pure, DOM-free MdBlock tree. */
export function parseMarkdown(text: string): MdBlock[] {
  if (!text) return [];
  const tokens = Lexer.lex(text, { gfm: true, breaks: true });
  const out: MdBlock[] = [];
  parseBlockTokens(tokens, 0, out);
  return out;
}

function appendTextWithBreaks(parent: Node, text: string): void {
  if (!text.includes("\n")) {
    parent.appendChild(document.createTextNode(text));
    return;
  }
  const parts = text.split("\n");
  for (let i = 0; i < parts.length; i++) {
    if (i > 0) parent.appendChild(document.createElement("br"));
    if (parts[i].length > 0) parent.appendChild(document.createTextNode(parts[i]));
  }
}

function appendRuns(parent: Node, runs: MdRun[]): void {
  let currentHref: string | null = null;
  let currentAnchor: HTMLAnchorElement | null = null;

  for (const run of runs) {
    const safeHref = run.link ? sanitizeLinkUrl(run.link) : null;
    if (safeHref !== currentHref) {
      currentHref = safeHref;
      if (safeHref) {
        currentAnchor = document.createElement("a");
        currentAnchor.className = "md-link";
        currentAnchor.setAttribute("href", safeHref);
        currentAnchor.setAttribute("target", "_blank");
        currentAnchor.setAttribute("rel", "noopener noreferrer");
        parent.appendChild(currentAnchor);
      } else {
        currentAnchor = null;
      }
    }

    let target: Node = currentAnchor ?? parent;
    if (run.bold) {
      const el = document.createElement("strong");
      target.appendChild(el);
      target = el;
    }
    if (run.italic) {
      const el = document.createElement("em");
      target.appendChild(el);
      target = el;
    }
    if (run.strikethrough) {
      const el = document.createElement("s");
      target.appendChild(el);
      target = el;
    }
    if (run.code) {
      const el = document.createElement("code");
      el.className = "md-code";
      target.appendChild(el);
      target = el;
    }
    appendTextWithBreaks(target, run.text);
  }
}

function renderCodeBlock(block: Extract<MdBlock, { kind: "code" }>, opts: Required<RenderMarkdownOptions>): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "md-pre";

  const head = document.createElement("div");
  head.className = "md-pre-head";

  const lang = document.createElement("span");
  lang.className = "md-pre-lang";
  lang.textContent = block.lang ?? "";

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "md-copy";
  copy.textContent = opts.copyLabel;

  let resetTimer: ReturnType<typeof setTimeout> | undefined;
  copy.addEventListener("click", () => {
    void Promise.resolve(opts.onCopy(block.code)).then((result) => {
      if (result === false) return;
      clearTimeout(resetTimer);
      copy.textContent = opts.copiedLabel;
      copy.classList.add("is-copied");
      resetTimer = setTimeout(() => {
        copy.textContent = opts.copyLabel;
        copy.classList.remove("is-copied");
      }, COPY_FEEDBACK_MS);
    });
  });

  head.append(lang, copy);

  const pre = document.createElement("pre");
  const code = document.createElement("code");
  code.textContent = block.code;
  pre.appendChild(code);

  wrap.append(head, pre);
  return wrap;
}

function renderTable(block: Extract<MdBlock, { kind: "table" }>): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "md-table-wrap";
  const table = document.createElement("table");
  table.className = "md-table";

  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  block.headers.forEach((cell, idx) => {
    const th = document.createElement("th");
    th.className = "md-th";
    const align = block.alignments[idx];
    if (align && align !== "none") th.style.textAlign = align;
    appendRuns(th, cell.runs);
    headRow.appendChild(th);
  });
  thead.appendChild(headRow);
  table.appendChild(thead);

  if (block.rows.length > 0) {
    const tbody = document.createElement("tbody");
    for (const row of block.rows) {
      const tr = document.createElement("tr");
      row.forEach((cell, idx) => {
        const td = document.createElement("td");
        td.className = "md-td";
        const align = block.alignments[idx];
        if (align && align !== "none") td.style.textAlign = align;
        appendRuns(td, cell.runs);
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
  }

  wrap.appendChild(table);
  return wrap;
}

function renderTaskMarker(checked: boolean): HTMLSpanElement {
  const box = document.createElement("span");
  box.className = "md-task-box";
  box.textContent = checked ? "[x]" : "[ ]";
  box.setAttribute("aria-hidden", "true");
  return box;
}

function renderBlocks(blocks: MdBlock[], parent: Node, opts: Required<RenderMarkdownOptions>, depth = 0): void {
  for (const block of blocks) {
    switch (block.kind) {
      case "heading": {
        const level = Math.min(6, Math.max(1, block.level));
        const h = document.createElement(`h${level}` as "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        h.className = `md-h md-h${level}`;
        const mark = document.createElement("span");
        mark.className = "md-h-mark";
        mark.textContent = "#".repeat(level);
        mark.setAttribute("aria-hidden", "true");
        h.appendChild(mark);
        appendRuns(h, block.runs);
        parent.appendChild(h);
        break;
      }
      case "paragraph": {
        const p = document.createElement("p");
        p.className = "md-p";
        appendRuns(p, block.runs);
        parent.appendChild(p);
        break;
      }
      case "code": {
        parent.appendChild(renderCodeBlock(block, opts));
        break;
      }
      case "blockquote": {
        if (depth >= MAX_DEPTH - 1) {
          renderBlocks(block.children, parent, opts, depth);
        } else {
          const bq = document.createElement("blockquote");
          bq.className = "md-quote";
          renderBlocks(block.children, bq, opts, depth + 1);
          parent.appendChild(bq);
        }
        break;
      }
      case "list": {
        if (depth >= MAX_DEPTH - 1) {
          for (const item of block.items) renderBlocks(item.children, parent, opts, depth);
          break;
        }
        let list: HTMLOListElement | HTMLUListElement;
        if (block.ordered) {
          const ol = document.createElement("ol");
          if (block.start !== 1) ol.start = block.start;
          list = ol;
        } else {
          list = document.createElement("ul");
        }
        list.className = "md-list";
        for (const item of block.items) {
          const li = document.createElement("li");
          li.className = "md-li";
          if (item.checked !== null) {
            li.classList.add("md-task");
            if (item.checked) li.classList.add("is-done");
            const marker = renderTaskMarker(item.checked);
            const [first, ...rest] = item.children;
            if (first && first.kind === "paragraph") {
              const p = document.createElement("p");
              p.className = "md-p";
              p.appendChild(marker);
              appendRuns(p, first.runs);
              li.appendChild(p);
              renderBlocks(rest, li, opts, depth + 1);
            } else {
              li.appendChild(marker);
              renderBlocks(item.children, li, opts, depth + 1);
            }
          } else {
            renderBlocks(item.children, li, opts, depth + 1);
          }
          list.appendChild(li);
        }
        parent.appendChild(list);
        break;
      }
      case "table": {
        parent.appendChild(renderTable(block));
        break;
      }
      case "hr": {
        const hr = document.createElement("hr");
        hr.className = "md-hr";
        parent.appendChild(hr);
        break;
      }
    }
  }
}

/** Render Markdown text into a DocumentFragment using safe DOM APIs only (no innerHTML). */
export function renderMarkdown(text: string, options?: RenderMarkdownOptions): DocumentFragment {
  const frag = document.createDocumentFragment();
  const blocks = parseMarkdown(text);
  if (blocks.length === 0) return frag;
  const opts: Required<RenderMarkdownOptions> = {
    copyLabel: options?.copyLabel ?? "Copy",
    copiedLabel: options?.copiedLabel ?? "Copied",
    onCopy: options?.onCopy ?? writeClipboard,
  };
  renderBlocks(blocks, frag, opts);
  return frag;
}
