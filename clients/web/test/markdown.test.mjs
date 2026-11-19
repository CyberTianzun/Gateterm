import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { execSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
execSync("npx esbuild src/markdown.ts --bundle --format=esm --outfile=test/.markdown.build.mjs", {
  cwd: join(here, ".."),
});
const { sanitizeLinkUrl, parseMarkdown, renderMarkdown } = await import("./.markdown.build.mjs");

function maxBlockDepth(blocks) {
  let max = 0;
  for (const b of blocks) {
    if (b.kind === "blockquote") {
      max = Math.max(max, 1 + maxBlockDepth(b.children));
    } else if (b.kind === "list") {
      for (const item of b.items) {
        max = Math.max(max, 1 + maxBlockDepth(item.children));
      }
    } else {
      max = Math.max(max, 1);
    }
  }
  return max;
}

function collectText(runs) {
  return runs.map((r) => r.text).join("");
}

test("sanitizeLinkUrl 允许 http/https/mailto 并剔除危险协议与控制字符", () => {
  assert.equal(sanitizeLinkUrl("https://example.com/path?a=1"), "https://example.com/path?a=1");
  assert.equal(sanitizeLinkUrl("  HTTP://EXAMPLE.COM  "), "HTTP://EXAMPLE.COM");
  assert.equal(sanitizeLinkUrl("MAILTO:user@example.com"), "MAILTO:user@example.com");

  assert.equal(sanitizeLinkUrl(undefined), null);
  assert.equal(sanitizeLinkUrl(""), null);
  assert.equal(sanitizeLinkUrl("   "), null);
  assert.equal(sanitizeLinkUrl("https://"), null);
  assert.equal(sanitizeLinkUrl("mailto:"), null);
  assert.equal(sanitizeLinkUrl("javascript:alert(1)"), null);
  assert.equal(sanitizeLinkUrl("JAVASCRIPT:alert(1)"), null);
  assert.equal(sanitizeLinkUrl("data:text/html,<script>alert(1)</script>"), null);
  assert.equal(sanitizeLinkUrl("file:///etc/passwd"), null);
  assert.equal(sanitizeLinkUrl("/relative/path"), null);
  assert.equal(sanitizeLinkUrl("https://example.com/\nbad"), null);
  assert.equal(sanitizeLinkUrl("java\tscript:alert(1)"), null);
});

test("parseMarkdown 解析标题 H1-H6 与保留单换行的段落", () => {
  const md = [
    "# H1",
    "## H2",
    "### H3",
    "#### H4",
    "##### H5",
    "###### H6",
    "",
    "line1",
    "line2  ",
    "line3",
  ].join("\n");

  const blocks = parseMarkdown(md);
  const headings = blocks.filter((b) => b.kind === "heading");
  assert.equal(headings.length, 6);
  assert.deepEqual(
    headings.map((h) => [h.level, collectText(h.runs)]),
    [
      [1, "H1"],
      [2, "H2"],
      [3, "H3"],
      [4, "H4"],
      [5, "H5"],
      [6, "H6"],
    ],
  );

  const para = blocks.find((b) => b.kind === "paragraph");
  assert.ok(para);
  assert.equal(collectText(para.runs), "line1\nline2\nline3");
});

test("parseMarkdown 解析嵌套行内样式并合并相邻同样式 run", () => {
  const blocks = parseMarkdown("**bold *both* ~~all~~** and `code`");
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "paragraph");
  assert.deepEqual(blocks[0].runs, [
    { text: "bold ", bold: true, italic: false, strikethrough: false, code: false, link: null },
    { text: "both", bold: true, italic: true, strikethrough: false, code: false, link: null },
    { text: " ", bold: true, italic: false, strikethrough: false, code: false, link: null },
    { text: "all", bold: true, italic: false, strikethrough: true, code: false, link: null },
    { text: " and ", bold: false, italic: false, strikethrough: false, code: false, link: null },
    { text: "code", bold: false, italic: false, strikethrough: false, code: true, link: null },
  ]);
});

test("parseMarkdown 解析围栏代码块与未闭合围栏", () => {
  const closed = parseMarkdown("```rust\nfn main() {}\n```");
  assert.deepEqual(closed, [{ kind: "code", lang: "rust", code: "fn main() {}" }]);

  const unclosed = parseMarkdown("```ts\nconst x = 1;");
  assert.deepEqual(unclosed, [{ kind: "code", lang: "ts", code: "const x = 1;" }]);

  const plain = parseMarkdown("```\nraw\n```");
  assert.deepEqual(plain, [{ kind: "code", lang: null, code: "raw" }]);
});

test("parseMarkdown 解析嵌套列表、任务列表与包含块级元素的引用块", () => {
  const md = [
    "- [x] **done** item",
    "- [ ] todo",
    "  - nested",
    "",
    "3. third",
    "4. fourth",
    "",
    "> quote",
    "> - q item",
    ">   ```js",
    ">   1",
    ">   ```",
  ].join("\n");

  const blocks = parseMarkdown(md);
  const taskList = blocks[0];
  assert.equal(taskList.kind, "list");
  assert.equal(taskList.ordered, false);
  assert.equal(taskList.items.length, 2);
  assert.equal(taskList.items[0].checked, true);
  assert.equal(taskList.items[1].checked, false);
  assert.ok(taskList.items[1].children.some((c) => c.kind === "list"));

  const ordered = blocks[1];
  assert.equal(ordered.kind, "list");
  assert.equal(ordered.ordered, true);
  assert.equal(ordered.start, 3);
  assert.equal(ordered.items.length, 2);
  assert.equal(ordered.items[0].checked, null);

  const quote = blocks[2];
  assert.equal(quote.kind, "blockquote");
  assert.equal(quote.children[0].kind, "paragraph");
  assert.equal(quote.children[1].kind, "list");
  const quoteListItem = quote.children[1].items[0];
  assert.ok(quoteListItem.children.some((c) => c.kind === "code" && c.lang === "js" && c.code === "1"));
});

test("parseMarkdown 解析 GFM 表格对齐与单元格行内样式", () => {
  const blocks = parseMarkdown("| A | B | C |\n|:---|:---:|---:|\n| **1** | `2` | 3 |");
  assert.equal(blocks.length, 1);
  const table = blocks[0];
  assert.equal(table.kind, "table");
  assert.deepEqual(table.alignments, ["left", "center", "right"]);
  assert.equal(table.headers.length, 3);
  assert.equal(collectText(table.headers[0].runs), "A");
  assert.equal(table.rows.length, 1);
  assert.deepEqual(table.rows[0][0].runs, [
    { text: "1", bold: true, italic: false, strikethrough: false, code: false, link: null },
  ]);
  assert.deepEqual(table.rows[0][1].runs, [
    { text: "2", bold: false, italic: false, strikethrough: false, code: true, link: null },
  ]);
});

test("parseMarkdown 安全性: 拦截危险协议、保留原始 HTML 为纯文本、图片转文本、裸链接、深嵌套限深", () => {
  const xssLinks = parseMarkdown("[ok](https://example.com) [xss](javascript:alert(1)) [data](data:text/html,x)");
  assert.equal(xssLinks[0].kind, "paragraph");
  assert.deepEqual(xssLinks[0].runs, [
    { text: "ok", bold: false, italic: false, strikethrough: false, code: false, link: "https://example.com" },
    { text: " xss data", bold: false, italic: false, strikethrough: false, code: false, link: null },
  ]);

  const htmlBlock = parseMarkdown("<script>alert(1)</script>");
  assert.equal(htmlBlock.length, 1);
  assert.equal(htmlBlock[0].kind, "paragraph");
  assert.equal(collectText(htmlBlock[0].runs), "<script>alert(1)</script>");

  const inlineHtml = parseMarkdown("before <img src=x onerror=alert(1)> after");
  assert.equal(inlineHtml[0].kind, "paragraph");
  assert.equal(collectText(inlineHtml[0].runs), "before <img src=x onerror=alert(1)> after");

  const img = parseMarkdown(
    "![alt](https://example.com/x.png) ![bad](javascript:alert(1)) ![](https://example.com/y.png) ![](javascript:alert(1))",
  );
  assert.equal(img[0].kind, "paragraph");
  assert.deepEqual(img[0].runs, [
    { text: "[alt]", bold: false, italic: false, strikethrough: false, code: false, link: "https://example.com/x.png" },
    { text: " [bad] ", bold: false, italic: false, strikethrough: false, code: false, link: null },
    {
      text: "[https://example.com/y.png]",
      bold: false,
      italic: false,
      strikethrough: false,
      code: false,
      link: "https://example.com/y.png",
    },
    { text: " [image]", bold: false, italic: false, strikethrough: false, code: false, link: null },
  ]);

  const linkedImg = parseMarkdown("[![alt](https://img.example/a.png)](https://target.example)");
  assert.deepEqual(linkedImg[0].runs, [
    { text: "[alt]", bold: false, italic: false, strikethrough: false, code: false, link: "https://target.example" },
  ]);

  const refDef = parseMarkdown("[foo]: https://example.com\n\nSee [foo].");
  assert.equal(refDef.length, 1);
  assert.equal(refDef[0].kind, "paragraph");
  assert.deepEqual(refDef[0].runs, [
    { text: "See ", bold: false, italic: false, strikethrough: false, code: false, link: null },
    { text: "foo", bold: false, italic: false, strikethrough: false, code: false, link: "https://example.com" },
    { text: ".", bold: false, italic: false, strikethrough: false, code: false, link: null },
  ]);

  const bare = parseMarkdown("see https://example.com/path).");
  assert.equal(bare[0].kind, "paragraph");
  assert.deepEqual(bare[0].runs, [
    { text: "see ", bold: false, italic: false, strikethrough: false, code: false, link: null },
    {
      text: "https://example.com/path",
      bold: false,
      italic: false,
      strikethrough: false,
      code: false,
      link: "https://example.com/path",
    },
    { text: ").", bold: false, italic: false, strikethrough: false, code: false, link: null },
  ]);

  const deepQuote = parseMarkdown(`${"> ".repeat(40)}leaf`);
  assert.ok(maxBlockDepth(deepQuote) <= 16, `嵌套深度超限: ${maxBlockDepth(deepQuote)}`);
  assert.ok(JSON.stringify(deepQuote).includes("leaf"));

  const deepList = parseMarkdown(Array.from({ length: 40 }, (_, i) => `${"  ".repeat(i)}- item ${i}`).join("\n"));
  assert.ok(maxBlockDepth(deepList) <= 16, `列表嵌套深度超限: ${maxBlockDepth(deepList)}`);
  assert.ok(JSON.stringify(deepList).includes("item 39"));
});

test("renderMarkdown 仅使用安全 DOM API（零 innerHTML）并正确处理复制按钮与安全性", async () => {
  function makeMockDoc() {
    class MockNode {
      constructor(nodeType, tagName = "") {
        this.nodeType = nodeType;
        this.tagName = tagName.toUpperCase();
        this.childNodes = [];
        this.attributes = new Map();
        this.style = {};
        this.listeners = new Map();
        this._className = "";
        this._text = "";
      }
      set innerHTML(_v) {
        throw new Error("innerHTML must never be used");
      }
      get innerHTML() {
        throw new Error("innerHTML must never be used");
      }
      set className(v) {
        this._className = v;
      }
      get className() {
        return this._className;
      }
      get classList() {
        const self = this;
        const list = () => self._className.split(/\s+/).filter(Boolean);
        return {
          add(...cls) {
            const set = new Set(list());
            for (const c of cls) set.add(c);
            self._className = [...set].join(" ");
          },
          remove(...cls) {
            const set = new Set(list());
            for (const c of cls) set.delete(c);
            self._className = [...set].join(" ");
          },
          contains(c) {
            return list().includes(c);
          },
        };
      }
      set textContent(v) {
        this._text = String(v);
        this.childNodes = [];
      }
      get textContent() {
        if (this.nodeType === 3) return this._text;
        return this._text + this.childNodes.map((c) => c.textContent).join("");
      }
      appendChild(child) {
        if (child.nodeType === 11) {
          this.childNodes.push(...child.childNodes);
          child.childNodes = [];
        } else {
          this.childNodes.push(child);
        }
        return child;
      }
      append(...nodes) {
        for (const n of nodes) this.appendChild(typeof n === "string" ? doc.createTextNode(n) : n);
      }
      setAttribute(k, v) {
        this.attributes.set(k, String(v));
      }
      getAttribute(k) {
        return this.attributes.get(k) ?? null;
      }
      addEventListener(type, fn) {
        if (!this.listeners.has(type)) this.listeners.set(type, []);
        this.listeners.get(type).push(fn);
      }
      click() {
        for (const fn of this.listeners.get("click") ?? []) fn();
      }
    }

    const doc = {
      createElement: (tag) => new MockNode(1, tag),
      createTextNode: (text) => {
        const n = new MockNode(3);
        n._text = String(text);
        return n;
      },
      createDocumentFragment: () => new MockNode(11),
    };
    return doc;
  }

  const prevDoc = globalThis.document;
  globalThis.document = makeMockDoc();
  try {
    const copied = [];
    const frag = renderMarkdown(
      [
        "# Title",
        "",
        "```js",
        "const a = 1;",
        "```",
        "",
        "- [x] done",
        "- [ ] todo",
        "",
        "> quoted",
        "",
        "| A | B |",
        "|:---|---:|",
        "| x | y |",
        "",
        "---",
        "",
        "[safe](https://example.com) [evil](javascript:alert(1)) ![pic](https://example.com/a.png)",
        "",
        "<script>alert(1)</script>",
      ].join("\n"),
      {
        copyLabel: "Copy",
        copiedLabel: "Copied",
        onCopy: (code) => {
          copied.push(code);
        },
      },
    );

    const allNodes = [];
    const walk = (n) => {
      allNodes.push(n);
      for (const c of n.childNodes) walk(c);
    };
    walk(frag);

    assert.equal(allNodes.some((n) => n.tagName === "SCRIPT"), false);
    assert.equal(allNodes.some((n) => n.tagName === "IMG"), false);

    const h1 = allNodes.find((n) => n.tagName === "H1");
    assert.ok(h1 && h1.classList.contains("md-h") && h1.classList.contains("md-h1"));

    assert.ok(allNodes.some((n) => n.tagName === "BLOCKQUOTE" && n.classList.contains("md-quote")));
    assert.ok(allNodes.some((n) => n.tagName === "HR" && n.classList.contains("md-hr")));
    assert.ok(allNodes.some((n) => n.tagName === "TABLE" && n.classList.contains("md-table")));
    assert.ok(allNodes.some((n) => n.tagName === "LI" && n.classList.contains("md-task") && n.classList.contains("is-done")));
    const ths = allNodes.filter((n) => n.tagName === "TH");
    assert.equal(ths[0].style.textAlign, "left");
    assert.equal(ths[1].style.textAlign, "right");

    const links = allNodes.filter((n) => n.tagName === "A");
    assert.equal(links.length, 2);
    assert.equal(links[0].getAttribute("href"), "https://example.com");
    assert.equal(links[0].getAttribute("rel"), "noopener noreferrer");
    assert.equal(links[1].textContent, "[pic]");

    const copyBtn = allNodes.find((n) => n.tagName === "BUTTON" && n.classList.contains("md-copy"));
    assert.ok(copyBtn);
    assert.equal(copyBtn.textContent, "Copy");
    copyBtn.click();
    await Promise.resolve();
    assert.deepEqual(copied, ["const a = 1;"]);
    assert.equal(copyBtn.textContent, "Copied");
  } finally {
    globalThis.document = prevDoc;
  }
});
