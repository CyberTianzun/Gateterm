/**
 * 设置面板:一个分 tab 的 modal。
 *
 * 所有改动**即时生效**(不设「保存」按钮)—— 主题和字体是所见即所得的东西,
 * 让用户改一格看一眼比改完一整屏再提交有用得多。持久化由 host.set 负责。
 *
 * 面板整个用 DOM API 搭,不用 innerHTML 拼用户数据:主题名来自导入的 JSON,
 * 拼字符串就是一个 XSS 口子。
 */
import type { ITheme } from "@xterm/xterm";
import { createModal } from "./modal";
import {
  type GateConfig,
  systemDefaultConfig,
  exportConfigJson,
  importConfigJson,
  sanitizeTheme,
  DEFAULT_FONT_FAMILY,
} from "./config";
import { BUILTIN_FONTS, ensureFontLoaded, matchBuiltinFont } from "./fonts";
import { knownThemes, loadCatalog, catalogLoaded, resolveTheme } from "./theme-catalog";
import { LOCALES, LOCALE_NAMES, type LocalePref, type MsgKey, localeTag, subscribe, t } from "./i18n";
import { createIcon } from "./icons";
import { writeClipboard } from "./clipboard";
import {
  type McpServerIdentity,
  buildMcpPresets,
  deriveEndpoints,
  deriveMcpServerKey,
  extractTitlePrefix,
  fetchMcpStatus,
} from "./mcp";
import { PROTO_VERSION } from "./protocol";
import { WEB_VERSION, fetchVersionStatus } from "./update";
import { buildPairingTarget, buildQrSvgModel, createQrSvgElement } from "./pairing";

export interface SettingsHost {
  get(): GateConfig;
  /** 应用并持久化。 */
  set(cfg: GateConfig): void;
  /** 打 `/api/` 时用的 bearer。扫码连接页、MCP 页与关于页用它。 */
  token(): string;
  /** 握手拿到的服务端系统标识("windows" / "macos" / "linux" / "")。 */
  serverOs?: () => string;
}

type Tab = "theme" | "font" | "terminal" | "ui" | "io" | "pairing" | "mcp" | "about";

const el = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
};

/** 一行「标签 + 控件」。 */
function row(label: string, control: HTMLElement, hint?: string): HTMLElement {
  const r = el("label", "set-row");
  const l = el("span", "set-label", label);
  if (hint) {
    const h = el("span", "set-hint", hint);
    l.appendChild(h);
  }
  r.append(l, control);
  return r;
}

/** 一行只读信息(不用 `<label>`,避免干扰右侧外链点击)。 */
function infoRow(label: string, value: HTMLElement, hint?: HTMLElement): HTMLElement {
  const r = el("div", "set-row");
  const l = el("span", "set-label", label);
  if (hint) l.appendChild(hint);
  r.append(l, value);
  return r;
}

function externalLink(href: string, text: string): HTMLAnchorElement {
  const a = el("a", "update-link");
  a.href = href;
  a.target = "_blank";
  a.rel = "noopener noreferrer";
  a.append(document.createTextNode(text), createIcon("arrow-up-right", "inline-icon"));
  return a;
}

function formatServerOs(os: string): string {
  const lower = os.trim().toLowerCase();
  if (lower === "windows") return "Windows";
  if (lower === "macos") return "macOS";
  if (lower === "linux") return "Linux";
  return os.trim();
}

function numberInput(value: number, min: number, max: number, step: number): HTMLInputElement {
  const i = el("input");
  i.type = "number";
  i.min = String(min);
  i.max = String(max);
  i.step = String(step);
  i.value = String(value);
  return i;
}

/** `label` 缺省时直接显示取值本身(字重那种 CSS 关键字不需要翻译)。 */
function select<T extends string>(
  options: readonly T[],
  value: T,
  label?: (o: T) => string,
): HTMLSelectElement {
  const s = el("select");
  for (const o of options) {
    const opt = el("option", undefined, label ? label(o) : o);
    opt.value = o;
    s.appendChild(opt);
  }
  s.value = value;
  return s;
}

function checkbox(value: boolean): HTMLInputElement {
  const i = el("input");
  i.type = "checkbox";
  i.checked = value;
  return i;
}

/** 主题色块条:8 个常用色 + 前景/背景,用来在不点开的情况下认出一个主题。 */
function swatch(theme: ITheme): HTMLElement {
  const s = el("span", "swatch");
  s.style.background = theme.background ?? "#000";
  for (const k of ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "foreground"] as const) {
    const d = el("i");
    d.style.background = theme[k] ?? "transparent";
    s.appendChild(d);
  }
  return s;
}

/** 光标形状枚举的显示名。block / outline 这些是 xterm 的内部取值,直接摆给
 *  用户看等于没说。字重那种本身就是 CSS 关键字的枚举则保持原样。 */
const CURSOR_LABELS: Record<string, MsgKey> = {
  block: "cursor.block",
  underline: "cursor.underline",
  bar: "cursor.bar",
  outline: "cursor.outline",
  none: "cursor.none",
};

const cursorLabel = (v: string): string => {
  const key = CURSOR_LABELS[v];
  return key ? t(key) : v;
};

/** 从字体栈里取第一个族名,用来做「系统装没装」的探测。 */
function firstFamily(stack: string): string {
  return (stack.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, "");
}

export function mountSettings(host: SettingsHost): { open: () => void } {
  const { dialog, setLabels } = createModal("settings-dialog", t("app.settings"), t("settings.close"));

  const tabsBar = el("div", "set-tabs");
  const body = el("div", "set-body");
  dialog.append(tabsBar, body);
  document.body.appendChild(dialog);

  let tab: Tab = "theme";
  // 标签文案在 renderTabs 里现取:切语言后重绘就跟着变了
  const TABS: [Tab, MsgKey][] = [
    ["theme", "settings.tab.theme"],
    ["font", "settings.tab.font"],
    ["terminal", "settings.tab.terminal"],
    ["ui", "settings.tab.ui"],
    ["io", "settings.tab.io"],
    ["pairing", "settings.tab.pairing"],
    ["mcp", "settings.tab.mcp"],
    ["about", "settings.tab.about"],
  ];

  /** 改一格配置:浅合并进当前配置并立即应用。 */
  const patch = (fn: (c: GateConfig) => GateConfig) => host.set(fn(host.get()));

  function renderTabs() {
    tabsBar.replaceChildren();
    for (const [id, key] of TABS) {
      const b = el("button", "set-tab" + (id === tab ? " active" : ""), t(key));
      b.addEventListener("click", () => {
        tab = id;
        render();
      });
      tabsBar.appendChild(b);
    }
  }

  /* ---------- 主题 ---------- */
  let themeFilter = "";

  function renderTheme(): HTMLElement {
    const wrap = el("div", "set-pane");
    const search = el("input", "set-search");
    search.type = "search";
    search.placeholder = catalogLoaded()
      ? t("settings.theme.search", { n: Object.keys(knownThemes()).length })
      : t("settings.theme.searchLoading");
    search.value = themeFilter;
    search.addEventListener("input", () => {
      themeFilter = search.value;
      grid.replaceChildren(...themeCards());
    });
    const grid = el("div", "theme-grid");

    function themeCards(): HTMLElement[] {
      const q = themeFilter.trim().toLowerCase();
      const current = host.get().theme.name;
      const entries = Object.entries(knownThemes())
        .filter(([name]) => q === "" || name.toLowerCase().includes(q))
        .sort(([a], [b]) => a.localeCompare(b));
      if (entries.length === 0) return [el("div", "set-empty", t("settings.theme.noMatch"))];
      return entries.slice(0, 400).map(([name, colors]) => {
        const card = el("button", "theme-card" + (name === current ? " active" : ""));
        card.append(swatch(colors), el("span", "theme-name", name));
        card.addEventListener("click", () => {
          patch((c) => ({ ...c, theme: { name, colors: sanitizeTheme(colors) } }));
          render();
        });
        return card;
      });
    }

    grid.replaceChildren(...themeCards());
    wrap.append(search, grid);

    if (!catalogLoaded()) {
      void loadCatalog().then(() => {
        if (tab === "theme") render();
      });
    }

    const note = el("div", "set-note", t("settings.theme.note"));
    wrap.appendChild(note);
    return wrap;
  }

  /* ---------- 字体 ---------- */
  function renderFont(): HTMLElement {
    const cfg = host.get();
    const wrap = el("div", "set-pane");

    const presetIds = [...BUILTIN_FONTS.map((f) => f.id), "custom"] as const;
    const exactPreset = BUILTIN_FONTS.find((f) => f.stack === cfg.font.family) ?? matchBuiltinFont(cfg.font.family);
    const activePresetId =
      exactPreset && (exactPreset.stack === cfg.font.family || exactPreset.url !== undefined)
        ? exactPreset.id
        : "custom";
    const presetSelect = select(
      presetIds,
      activePresetId,
      (id) => BUILTIN_FONTS.find((f) => f.id === id)?.label ?? t("settings.font.presetCustom"),
    );
    presetSelect.addEventListener("change", () => {
      const picked = BUILTIN_FONTS.find((f) => f.id === presetSelect.value);
      if (picked) {
        patch((c) => ({ ...c, font: { ...c.font, family: picked.stack } }));
        render();
      }
    });

    const family = el("input", "set-wide");
    family.type = "text";
    family.value = cfg.font.family;
    family.spellcheck = false;
    const avail = el("div", "set-note");
    const checkAvail = (stack: string) => {
      const first = firstFamily(stack);
      const generic = ["monospace", "ui-monospace", "serif", "sans-serif", "system-ui"].includes(first);
      const isBuiltin = BUILTIN_FONTS.some(
        (f) => f.url && f.family.toLowerCase() === first.toLowerCase(),
      );
      if (first === "") {
        avail.replaceChildren();
        avail.className = "set-note";
      } else if (generic || isBuiltin || document.fonts?.check?.(`12px "${first}"`)) {
        avail.replaceChildren(
          createIcon("check", "inline-icon"),
          document.createTextNode(t("settings.font.available", { name: first })),
        );
        avail.className = "set-note ok";
      } else {
        avail.replaceChildren(
          createIcon("alert-triangle", "inline-icon"),
          document.createTextNode(t("settings.font.missing", { name: first })),
        );
        avail.className = "set-note warn";
      }
    };
    checkAvail(cfg.font.family);
    void ensureFontLoaded(cfg.font.family).then(() => checkAvail(family.value));
    family.addEventListener("change", () => {
      patch((c) => ({ ...c, font: { ...c.font, family: family.value } }));
      // sanitize 可能改写了输入(剔非法字符/空值回退),回读一次让输入框说实话
      family.value = host.get().font.family;
      checkAvail(family.value);
      render();
    });

    const reset = el("button", undefined, t("settings.font.reset"));
    reset.addEventListener("click", () => {
      patch((c) => ({ ...c, font: { ...c.font, family: DEFAULT_FONT_FAMILY } }));
      render();
    });

    const size = numberInput(cfg.font.size, 6, 48, 1);
    size.addEventListener("input", () =>
      patch((c) => ({ ...c, font: { ...c.font, size: Number(size.value) } })),
    );
    const lh = numberInput(cfg.font.lineHeight, 0.8, 3, 0.05);
    lh.addEventListener("input", () =>
      patch((c) => ({ ...c, font: { ...c.font, lineHeight: Number(lh.value) } })),
    );
    const ls = numberInput(cfg.font.letterSpacing, -5, 10, 0.5);
    ls.addEventListener("input", () =>
      patch((c) => ({ ...c, font: { ...c.font, letterSpacing: Number(ls.value) } })),
    );
    const weights = ["normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"] as const;
    const w = select(weights, String(cfg.font.weight) as (typeof weights)[number]);
    w.addEventListener("change", () =>
      patch((c) => ({ ...c, font: { ...c.font, weight: w.value as never } })),
    );
    const wb = select(weights, String(cfg.font.weightBold) as (typeof weights)[number]);
    wb.addEventListener("change", () =>
      patch((c) => ({ ...c, font: { ...c.font, weightBold: wb.value as never } })),
    );

    const preview = el("div", "font-preview");
    preview.style.fontFamily = cfg.font.family;
    preview.style.fontSize = `${cfg.font.size}px`;
    preview.style.lineHeight = String(cfg.font.lineHeight);
    preview.style.letterSpacing = `${cfg.font.letterSpacing}px`;
    preview.style.background = cfg.theme.colors.background ?? "#000";
    preview.style.color = cfg.theme.colors.foreground ?? "#fff";
    preview.textContent = "iIlL1 oO0 `'\" ─┼┤├ 中文对齐 => != >= 0x1F ~$#@";

    wrap.append(
      row(t("settings.font.preset"), presetSelect, t("settings.font.presetHint")),
      row(t("settings.font.family"), family, t("settings.font.familyHint")),
      avail,
      row("", reset),
      row(t("settings.font.size"), size, t("unit.px")),
      row(t("settings.font.lineHeight"), lh, t("unit.times")),
      row(t("settings.font.letterSpacing"), ls, t("unit.px")),
      row(t("settings.font.weight"), w),
      row(t("settings.font.weightBold"), wb),
      preview,
    );
    return wrap;
  }

  /* ---------- 终端行为 ---------- */
  function renderTerminal(): HTMLElement {
    const cfg = host.get();
    const wrap = el("div", "set-pane");

    const cs = select(["block", "underline", "bar"] as const, cfg.terminal.cursorStyle, cursorLabel);
    cs.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, cursorStyle: cs.value as never } })),
    );
    const cis = select(
      ["outline", "block", "bar", "underline", "none"] as const,
      cfg.terminal.cursorInactiveStyle,
      cursorLabel,
    );
    cis.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, cursorInactiveStyle: cis.value as never } })),
    );
    const blink = checkbox(cfg.terminal.cursorBlink);
    blink.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, cursorBlink: blink.checked } })),
    );
    const sb = numberInput(cfg.terminal.scrollback, 0, 200_000, 500);
    sb.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, scrollback: Number(sb.value) } })),
    );
    const blockMouse = checkbox(cfg.terminal.blockMouseEvents);
    blockMouse.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, blockMouseEvents: blockMouse.checked } })),
    );
    const glyphs = checkbox(cfg.terminal.customGlyphs);
    glyphs.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, customGlyphs: glyphs.checked } })),
    );
    const contrast = numberInput(cfg.terminal.minimumContrastRatio, 1, 21, 0.5);
    contrast.addEventListener("change", () =>
      patch((c) => ({ ...c, terminal: { ...c.terminal, minimumContrastRatio: Number(contrast.value) } })),
    );
    const boldBright = checkbox(cfg.terminal.drawBoldTextInBrightColors);
    boldBright.addEventListener("change", () =>
      patch((c) => ({
        ...c,
        terminal: { ...c.terminal, drawBoldTextInBrightColors: boldBright.checked },
      })),
    );

    wrap.append(
      row(t("settings.term.cursorStyle"), cs),
      row(t("settings.term.cursorInactive"), cis),
      row(t("settings.term.cursorBlink"), blink),
      row(t("settings.term.scrollback"), sb, t("unit.lines")),
      row(t("settings.term.blockMouseEvents"), blockMouse, t("settings.term.blockMouseEventsHint")),
      row(t("settings.term.customGlyphs"), glyphs, t("settings.term.customGlyphsHint")),
      row(t("settings.term.contrast"), contrast, t("settings.term.contrastHint")),
      row(t("settings.term.boldBright"), boldBright),
    );
    return wrap;
  }

  /* ---------- 界面 ---------- */

  /**
   * 语言排在第一行,而且整个 tab 就叫「界面」。
   *
   * 之前这几项挂在「终端」tab 的一个分隔线下面 —— 找不到。这三个开关本来也
   * 不是终端行为(它们管的是侧边栏、渲染器、配色外溢),搬出来两件事一起修。
   */
  function renderUi(): HTMLElement {
    const cfg = host.get();
    const wrap = el("div", "set-pane");

    const langs: readonly LocalePref[] = ["auto", ...LOCALES];
    const lang = select(langs, cfg.ui.locale, (l) =>
      l === "auto" ? t("settings.ui.languageAuto") : LOCALE_NAMES[l],
    );
    lang.addEventListener("change", () =>
      patch((c) => ({ ...c, ui: { ...c.ui, locale: lang.value as LocalePref } })),
    );

    const webgl = checkbox(cfg.ui.webgl);
    webgl.addEventListener("change", () =>
      patch((c) => ({ ...c, ui: { ...c.ui, webgl: webgl.checked } })),
    );

    wrap.append(
      row(t("settings.ui.language"), lang, t("settings.ui.languageHint")),
      row(t("settings.ui.webgl"), webgl, t("settings.ui.webglHint")),
    );
    return wrap;
  }

  /* ---------- 导入 / 导出 ---------- */
  function renderIo(): HTMLElement {
    const wrap = el("div", "set-pane");
    const ta = el("textarea", "set-json");
    ta.spellcheck = false;
    ta.value = exportConfigJson(host.get());
    const status = el("div", "set-note");

    const say = (msg: string, kind: "ok" | "warn" | "" = "") => {
      status.textContent = msg;
      status.className = "set-note" + (kind ? ` ${kind}` : "");
    };

    const apply = (text: string, source: string) => {
      let result;
      try {
        result = importConfigJson(text, resolveTheme);
      } catch (err) {
        say(t("settings.io.parseFail", { source, error: (err as Error).message }), "warn");
        return;
      }
      host.set(result.config);
      ta.value = exportConfigJson(result.config);
      if (result.warnings.length === 0) {
        say(t("settings.io.ok", { source }), "ok");
      } else {
        say(
          t("settings.io.okWarn", { source, n: result.warnings.length }) +
            "\n· " +
            result.warnings.join("\n· "),
          "warn",
        );
      }
    };

    const applyBtn = el("button", undefined, t("settings.io.apply"));
    applyBtn.addEventListener("click", () => apply(ta.value, t("settings.io.srcImport")));

    const download = el("button", undefined, t("settings.io.download"));
    download.addEventListener("click", () => {
      const blob = new Blob([exportConfigJson(host.get())], { type: "application/json" });
      const a = el("a");
      a.href = URL.createObjectURL(blob);
      a.download = "gateterm-config.json";
      a.click();
      // 立刻 revoke 会让部分浏览器来不及发起下载,挂到下一帧之后
      setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
    });

    const file = el("input");
    file.type = "file";
    file.accept = "application/json,.json";
    file.hidden = true;
    file.addEventListener("change", async () => {
      const f = file.files?.[0];
      if (!f) return;
      apply(await f.text(), t("settings.io.srcFile"));
      file.value = "";
    });
    const upload = el("button", undefined, t("settings.io.upload"));
    upload.addEventListener("click", () => file.click());

    const reset = el("button", "danger", t("settings.io.reset"));
    reset.addEventListener("click", () => {
      if (!confirm(t("settings.io.resetConfirm"))) return;
      // 和首次打开走同一条路:重新读一次系统亮暗,而不是钉死深色
      host.set(systemDefaultConfig());
      ta.value = exportConfigJson(host.get());
      say(t("settings.io.resetDone", { name: host.get().theme.name }), "ok");
    });

    const bar = el("div", "set-bar");
    bar.append(applyBtn, download, upload, reset, file);
    wrap.append(
      el("div", "set-note", t("settings.io.note")),
      ta,
      bar,
      status,
    );
    return wrap;
  }

  /* ---------- 扫码连接 ---------- */

  function pairingNoticeText(reason: Exclude<ReturnType<typeof buildPairingTarget>, { ok: true }>): string {
    switch (reason.reason) {
      case "loopback":
        return t("pairing.notice.loopback", { host: reason.host ?? window.location.hostname });
      case "missing-token":
        return t("pairing.notice.missingToken");
      case "unsupported-scheme":
        return t("pairing.notice.unsupportedScheme");
      case "userinfo-not-allowed":
        return t("pairing.notice.userinfo");
      case "invalid-url":
        return t("pairing.notice.invalidUrl");
    }
  }

  function renderPairing(): HTMLElement {
    const wrap = el("div", "set-pane pairing-pane");
    const target = buildPairingTarget(
      window.location.href,
      host.token(),
      extractTitlePrefix(document.title),
    );

    if (!target.ok) {
      const notice = el("div", "pairing-notice");
      notice.setAttribute("role", "status");
      notice.append(
        el("div", "pairing-notice-title", t("pairing.notice.title")),
        el("p", "pairing-notice-body", pairingNoticeText(target)),
      );
      wrap.append(notice);
      return wrap;
    }

    const model = buildQrSvgModel(target.url);
    const card = el("section", "pairing-card");
    const layout = el("div", "pairing-layout");

    if (model.ok) {
      const qrBox = el("div", "pairing-qr-box");
      qrBox.appendChild(createQrSvgElement(model, t("pairing.qrAlt")));
      layout.appendChild(qrBox);
    } else {
      layout.appendChild(el("div", "pairing-qr-fallback", t("pairing.tooLong")));
    }

    const info = el("div", "pairing-info");
    info.append(
      el("div", "pairing-title", t("pairing.title")),
      el("div", "pairing-desc", t("pairing.desc")),
    );

    const urlField = el("div", "pairing-field");
    urlField.append(el("span", "pairing-field-label", t("pairing.addressLabel")));
    const urlRow = el("div", "pairing-url-row");
    const urlCode = el("code", "pairing-url", target.displayUrl);

    const copyBtn = el("button", "pairing-copy-btn", t("pairing.copyLink"));
    copyBtn.type = "button";
    const copyError = el("div", "pairing-copy-error");
    copyError.hidden = true;
    copyError.setAttribute("role", "status");

    let resetTimer: ReturnType<typeof setTimeout> | undefined;
    copyBtn.addEventListener("click", () => {
      void writeClipboard(target.url).then((ok) => {
        if (!ok) {
          copyError.textContent = t("pairing.copyFailed");
          copyError.hidden = false;
          return;
        }
        copyError.hidden = true;
        clearTimeout(resetTimer);
        copyBtn.textContent = t("pairing.copied");
        copyBtn.classList.add("is-copied");
        resetTimer = setTimeout(() => {
          copyBtn.textContent = t("pairing.copyLink");
          copyBtn.classList.remove("is-copied");
        }, 1600);
      });
    });

    urlRow.append(urlCode, copyBtn);
    urlField.append(urlRow, copyError);

    const steps = el("ol", "pairing-steps");
    steps.append(
      el("li", undefined, t("pairing.step1")),
      el("li", undefined, t("pairing.step2")),
    );

    const security = el("div", "pairing-security", t("pairing.securityHint"));
    info.append(urlField, steps, security);
    layout.appendChild(info);
    card.appendChild(layout);
    wrap.appendChild(card);
    return wrap;
  }

  /* ---------- MCP 接入 ---------- */
  let revealMcpToken = false;
  let mcpRenderSeq = 0;

  function formatMcpNodeMeta(server?: McpServerIdentity): string {
    if (!server) return "";
    const label = server.name?.trim() || server.hostname?.trim() || "";
    const os = server.os?.trim() || "";
    const arch = server.arch?.trim() || "";
    const platform = [os, arch].filter(Boolean).join("/");
    return [label, platform].filter(Boolean).join(" · ");
  }

  function renderMcp(): HTMLElement {
    const wrap = el("div", "set-pane mcp-pane");
    const renderSeq = ++mcpRenderSeq;
    const endpoints = deriveEndpoints(window.location.href);

    let defaultServerName = deriveMcpServerKey(
      [extractTitlePrefix(document.title)],
      endpoints.apiBaseUrl,
    );
    let customServerName = "";
    const effectiveServerName = () => customServerName.trim() || defaultServerName;

    const presetsFor = (masked: boolean) =>
      buildMcpPresets({
        mcpUrl: endpoints.mcpUrl,
        apiBaseUrl: endpoints.apiBaseUrl,
        token: host.token(),
        serverName: effectiveServerName(),
        masked,
      });

    const toolbar = el("div", "mcp-toolbar");
    const badge = el("span", "mcp-badge", t("mcp.status.loading"));
    badge.dataset.state = "loading";
    const protoSpan = el("span", "mcp-protocols");
    protoSpan.hidden = true;
    const nodeMeta = el("span", "mcp-node-meta");
    nodeMeta.hidden = true;
    const revealBtn = el("button", "mcp-reveal");
    revealBtn.type = "button";
    toolbar.append(badge, protoSpan, nodeMeta, revealBtn);

    const banner = el("div", "mcp-banner");
    banner.hidden = true;
    banner.setAttribute("role", "status");

    const securityHint = el("div", "mcp-security", t("mcp.securityHint"));
    const copyError = el("div", "mcp-copy-error");
    copyError.hidden = true;
    copyError.setAttribute("role", "status");

    const nameRow = el("div", "mcp-name-row");
    const nameLabel = el("label", "mcp-name-label", t("mcp.serverName.label"));
    const nameInput = el("input", "mcp-name-input");
    nameInput.type = "text";
    nameInput.id = "mcp-server-name-input";
    nameInput.placeholder = defaultServerName;
    nameInput.autocomplete = "off";
    nameInput.spellcheck = false;
    nameLabel.htmlFor = nameInput.id;
    const nameHint = el("div", "mcp-name-hint", t("mcp.serverName.hint"));
    nameRow.append(nameLabel, nameInput, nameHint);

    const COPY_FEEDBACK_MS = 1600;
    const makeCopyButton = (getCopyText: () => string): HTMLButtonElement => {
      const btn = el("button", "mcp-copy-btn", t("mcp.copy"));
      btn.type = "button";
      let resetTimer: ReturnType<typeof setTimeout> | undefined;
      btn.addEventListener("click", () => {
        void writeClipboard(getCopyText()).then((ok) => {
          if (!ok) {
            copyError.textContent = t("mcp.copyFailed");
            copyError.hidden = false;
            return;
          }
          copyError.hidden = true;
          clearTimeout(resetTimer);
          btn.textContent = t("mcp.copied");
          btn.classList.add("is-copied");
          resetTimer = setTimeout(() => {
            btn.textContent = t("mcp.copy");
            btn.classList.remove("is-copied");
          }, COPY_FEEDBACK_MS);
        });
      });
      return btn;
    };

    const makeCodePre = (): { pre: HTMLPreElement; code: HTMLElement } => {
      const pre = el("pre", "mcp-code");
      const code = el("code");
      pre.appendChild(code);
      return { pre, code };
    };

    // Card 1: Endpoint & Header
    const endpointCard = el("section", "mcp-card");
    const endpointHead = el("div", "mcp-card-head");
    endpointHead.append(el("div", "mcp-card-title", t("mcp.card.endpoint.title")));
    const endpointDesc = el("div", "mcp-card-desc", t("mcp.card.endpoint.desc"));

    const urlField = el("div", "mcp-field");
    urlField.append(el("span", "mcp-field-label", t("mcp.field.url")));
    const urlRow = el("div", "mcp-field-row");
    const urlVal = el("code", "mcp-inline");
    urlRow.append(urlVal, makeCopyButton(() => presetsFor(false).mcpUrl));
    urlField.appendChild(urlRow);

    const authField = el("div", "mcp-field");
    authField.append(el("span", "mcp-field-label", t("mcp.field.authHeader")));
    const authRow = el("div", "mcp-field-row");
    const authVal = el("code", "mcp-inline");
    authRow.append(authVal, makeCopyButton(() => presetsFor(false).authHeader));
    authField.appendChild(authRow);

    endpointCard.append(endpointHead, endpointDesc, urlField, authField);

    // Card 2: Generic Streamable HTTP MCP JSON
    const jsonCard = el("section", "mcp-card");
    const jsonHead = el("div", "mcp-card-head");
    jsonHead.append(
      el("div", "mcp-card-title", t("mcp.card.json.title")),
      makeCopyButton(() => presetsFor(false).genericJson),
    );
    const jsonDesc = el("div", "mcp-card-desc", t("mcp.card.json.desc"));
    const jsonBlock = makeCodePre();
    jsonCard.append(jsonHead, jsonDesc, jsonBlock.pre);

    // Card 3: Coding Agents (Claude Code + Codex)
    const agentsCard = el("section", "mcp-card");
    const agentsHead = el("div", "mcp-card-head");
    agentsHead.append(el("div", "mcp-card-title", t("mcp.card.agents.title")));
    const agentsDesc = el("div", "mcp-card-desc", t("mcp.card.agents.desc"));

    const claudeSub = el("div", "mcp-subhead");
    claudeSub.append(
      el("span", "mcp-sublabel", t("mcp.agent.claudeCode")),
      makeCopyButton(() => presetsFor(false).claudeCodeCmd),
    );
    const claudeBlock = makeCodePre();

    const codexSub = el("div", "mcp-subhead");
    codexSub.append(
      el("span", "mcp-sublabel", t("mcp.agent.codex")),
      makeCopyButton(() => presetsFor(false).codexToml),
    );
    const codexBlock = makeCodePre();

    agentsCard.append(agentsHead, agentsDesc, claudeSub, claudeBlock.pre, codexSub, codexBlock.pre);

    // Card 4: HTTP API / curl quick reference
    const curlCard = el("section", "mcp-card");
    const curlHead = el("div", "mcp-card-head");
    curlHead.append(
      el("div", "mcp-card-title", t("mcp.card.curl.title")),
      makeCopyButton(() => presetsFor(false).curlExamples),
    );
    const curlDesc = el("div", "mcp-card-desc", t("mcp.card.curl.desc"));
    const curlBlock = makeCodePre();
    curlCard.append(curlHead, curlDesc, curlBlock.pre);

    const updatePreviews = () => {
      const p = presetsFor(!revealMcpToken);
      revealBtn.textContent = revealMcpToken ? t("mcp.token.hide") : t("mcp.token.reveal");
      revealBtn.setAttribute("aria-pressed", String(revealMcpToken));
      urlVal.textContent = p.mcpUrl;
      authVal.textContent = p.authHeader;
      jsonBlock.code.textContent = p.genericJson;
      claudeBlock.code.textContent = p.claudeCodeCmd;
      codexBlock.code.textContent = p.codexToml;
      curlBlock.code.textContent = p.curlExamples;
    };

    revealBtn.addEventListener("click", () => {
      revealMcpToken = !revealMcpToken;
      updatePreviews();
    });
    nameInput.addEventListener("input", () => {
      customServerName = nameInput.value;
      updatePreviews();
    });
    updatePreviews();

    wrap.append(toolbar, banner, securityHint, copyError, nameRow, endpointCard, jsonCard, agentsCard, curlCard);

    void fetchMcpStatus(host.token(), undefined, endpoints.apiBaseUrl).then((res) => {
      if (renderSeq !== mcpRenderSeq || !wrap.isConnected) return;
      const serverIdentity = res.state === "enabled" || res.state === "disabled" ? res.server : undefined;
      const nodeMetaText = formatMcpNodeMeta(serverIdentity);
      nodeMeta.textContent = nodeMetaText;
      nodeMeta.hidden = !nodeMetaText;
      if (serverIdentity) {
        defaultServerName = deriveMcpServerKey(
          [serverIdentity.name, serverIdentity.hostname, extractTitlePrefix(document.title)],
          endpoints.apiBaseUrl,
        );
        nameInput.placeholder = defaultServerName;
        updatePreviews();
      }
      if (res.state === "enabled") {
        badge.dataset.state = "enabled";
        badge.textContent = t("mcp.status.enabled");
        if (res.protocolVersions.length > 0) {
          protoSpan.textContent = t("mcp.status.enabledProtocols", {
            versions: res.protocolVersions.join(", "),
          });
          protoSpan.hidden = false;
        } else {
          protoSpan.hidden = true;
        }
        banner.hidden = true;
      } else if (res.state === "disabled") {
        badge.dataset.state = "disabled";
        badge.textContent = t("mcp.status.disabled");
        protoSpan.hidden = true;
        banner.dataset.state = "disabled";
        banner.textContent = t("mcp.status.disabledBanner");
        banner.hidden = false;
      } else if (res.state === "unsupported") {
        badge.dataset.state = "unsupported";
        badge.textContent = t("mcp.status.unsupported");
        protoSpan.hidden = true;
        banner.dataset.state = "unsupported";
        banner.textContent = t("mcp.status.unsupportedBanner");
        banner.hidden = false;
      } else {
        badge.dataset.state = "error";
        badge.textContent = t("mcp.status.error");
        protoSpan.hidden = true;
        banner.dataset.state = "error";
        banner.textContent = t("mcp.status.errorBanner");
        banner.hidden = false;
      }
    });

    return wrap;
  }

  /* ---------- 关于 ---------- */
  function renderAbout(): HTMLElement {
    const wrap = el("div", "set-pane");

    const hero = el("div", "about-hero");
    const brand = el("div", "about-brand");
    brand.append(el("span", undefined, "gateterm"));
    const versionPill = el("span", "about-version-pill", WEB_VERSION ? `v${WEB_VERSION}` : "");
    brand.append(versionPill);
    hero.append(brand, el("div", "about-tagline", t("settings.about.tagline")));

    const webVerVal = el("span", "about-val about-mono", WEB_VERSION ? `v${WEB_VERSION}` : "—");
    const serverVerVal = el("span", "about-val about-mono", t("settings.about.loading"));
    const updateHint = el("span", "set-hint");
    updateHint.hidden = true;
    const updateVal = el("span", "about-val", t("settings.about.loading"));
    const protoVal = el("span", "about-val about-mono", `v${PROTO_VERSION}`);
    const licenseVal = el("span", "about-val", "MIT");

    const linksBox = el("div", "about-links");
    linksBox.append(
      externalLink("https://github.com/CyberTianzun/Gateterm", t("settings.about.repo")),
      externalLink("https://github.com/CyberTianzun/Gateterm/releases", t("settings.about.releases")),
      externalLink("https://github.com/CyberTianzun/Gateterm/issues", t("settings.about.issues")),
    );

    wrap.append(
      hero,
      infoRow(t("settings.about.webVersion"), webVerVal),
      infoRow(t("settings.about.serverVersion"), serverVerVal),
      infoRow(t("settings.about.updateStatus"), updateVal, updateHint),
      infoRow(t("settings.about.protocol"), protoVal),
      infoRow(t("settings.about.license"), licenseVal),
      infoRow(t("settings.about.links"), linksBox),
    );

    void fetchVersionStatus(host.token())
      .then((status) => {
        const osLabel = formatServerOs(host.serverOs?.() ?? "");
        if (status.currentVersion) {
          serverVerVal.textContent = osLabel
            ? `v${status.currentVersion} (${osLabel})`
            : `v${status.currentVersion}`;
          if (!versionPill.textContent) versionPill.textContent = `v${status.currentVersion}`;
        } else {
          serverVerVal.textContent = osLabel
            ? `${t("settings.about.unknown")} (${osLabel})`
            : t("settings.about.unknown");
        }

        if (status.update) {
          updateVal.replaceChildren(
            externalLink(status.update.url, t("update.available", { version: status.update.version })),
          );
          updateHint.hidden = true;
        } else if (status.checkedAt !== null) {
          updateVal.textContent = t("settings.about.upToDate");
          const time = new Date(status.checkedAt * 1000).toLocaleString(localeTag());
          updateHint.textContent = t("settings.about.checkedAt", { time });
          updateHint.hidden = false;
        } else if (status.currentVersion !== null) {
          updateVal.textContent = t("settings.about.checkPending");
          updateHint.hidden = true;
        } else {
          updateVal.textContent = "—";
          updateHint.hidden = true;
        }
      })
      .catch(() => {
        serverVerVal.textContent = t("settings.about.unavailable");
        updateVal.textContent = t("settings.about.unavailable");
        updateHint.hidden = true;
      });

    return wrap;
  }

  function render() {
    setLabels(t("app.settings"), t("settings.close"));
    renderTabs();
    const pane =
      tab === "theme" ? renderTheme()
      : tab === "font" ? renderFont()
      : tab === "terminal" ? renderTerminal()
      : tab === "ui" ? renderUi()
      : tab === "io" ? renderIo()
      : tab === "pairing" ? renderPairing()
      : tab === "mcp" ? renderMcp()
      : renderAbout();
    body.replaceChildren(pane);
  }

  // 语言是在这个面板里改的,所以改完必须原地重绘一次,不然用户看着的还是旧文案。
  subscribe(() => {
    if (dialog.open) render();
  });

  return {
    open() {
      revealMcpToken = false;
      render();
      dialog.showModal();
    },
  };
}
