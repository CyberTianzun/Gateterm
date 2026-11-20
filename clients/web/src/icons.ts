/**
 * 统一的内联单色线性 SVG 图标表。
 *
 * 所有 UI 按钮与内联状态图标都从这里生成,彻底脱离宿主系统的 Unicode 符号字体
 * 回退(在 Omarchy 等精简字体配置的 Linux 发行版上,⊞/⚙/✎/✕/☰/↗/✓/⚠ 等字符会
 * 退化成豆腐块或被劫持为彩色 emoji)。
 */

const SVG_NS = "http://www.w3.org/2000/svg";

interface SvgShape {
  readonly tag: "path" | "rect" | "circle";
  readonly attrs: Readonly<Record<string, string>>;
}

const ICONS = {
  "sidebar-pin-right": [
    { tag: "rect", attrs: { x: "3", y: "4", width: "18", height: "16", rx: "2" } },
    { tag: "path", attrs: { d: "M15 4v16m-8-11 3 3-3 3" } },
  ],
  "sidebar-hide": [
    { tag: "rect", attrs: { x: "3", y: "4", width: "18", height: "16", rx: "2" } },
    { tag: "path", attrs: { d: "M9 4v16m5-11 3 3-3 3" } },
  ],
  "sidebar-show": [
    { tag: "rect", attrs: { x: "3", y: "4", width: "18", height: "16", rx: "2" } },
    { tag: "path", attrs: { d: "M9 4v16m5-11-3 3 3 3" } },
  ],
  history: [
    { tag: "path", attrs: { d: "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" } },
    { tag: "path", attrs: { d: "M3 3v5h5M12 7v5l4 2" } },
  ],
  grid: [
    { tag: "rect", attrs: { x: "3", y: "3", width: "7", height: "7", rx: "1" } },
    { tag: "rect", attrs: { x: "14", y: "3", width: "7", height: "7", rx: "1" } },
    { tag: "rect", attrs: { x: "3", y: "14", width: "7", height: "7", rx: "1" } },
    { tag: "rect", attrs: { x: "14", y: "14", width: "7", height: "7", rx: "1" } },
  ],
  settings: [
    {
      tag: "path",
      attrs: {
        d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z",
      },
    },
    { tag: "circle", attrs: { cx: "12", cy: "12", r: "3" } },
  ],
  plus: [{ tag: "path", attrs: { d: "M12 4.5v15M4.5 12h15" } }],
  menu: [{ tag: "path", attrs: { d: "M4 6h16M4 12h16M4 18h16" } }],
  chat: [
    { tag: "path", attrs: { d: "M21 14a3 3 0 0 1-3 3H8l-5 4V6a3 3 0 0 1 3-3h12a3 3 0 0 1 3 3z" } },
    { tag: "path", attrs: { d: "M7 8h10M7 12h7" } },
  ],
  terminal: [
    { tag: "rect", attrs: { x: "3", y: "4", width: "18", height: "16", rx: "2" } },
    { tag: "path", attrs: { d: "m7 9 3 3-3 3m6 0h4" } },
  ],
  pencil: [
    { tag: "path", attrs: { d: "M17 3a2.83 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" } },
    { tag: "path", attrs: { d: "m15 5 4 4" } },
  ],
  close: [{ tag: "path", attrs: { d: "M18 6 6 18M6 6l12 12" } }],
  filter: [{ tag: "path", attrs: { d: "M3 5h18l-7 8v5l-4 2v-7L3 5z" } }],
  "arrow-up-right": [{ tag: "path", attrs: { d: "M7 17 17 7M7 7h10v10" } }],
  check: [{ tag: "path", attrs: { d: "M20 6 9 17l-5-5" } }],
  "alert-triangle": [
    { tag: "path", attrs: { d: "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" } },
    { tag: "path", attrs: { d: "M12 9v4M12 17h.01" } },
  ],
} as const satisfies Record<string, readonly SvgShape[]>;

export type UiIconName = keyof typeof ICONS;

export const UI_ICON_NAMES = Object.keys(ICONS) as readonly UiIconName[];

const SVG_BASE_ATTRS: Readonly<Record<string, string>> = {
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  "stroke-width": "1.8",
  "stroke-linecap": "round",
  "stroke-linejoin": "round",
  "aria-hidden": "true",
  focusable: "false",
};

function serializeAttrs(attrs: Readonly<Record<string, string>>): string {
  return Object.entries(attrs)
    .map(([k, v]) => `${k}="${v}"`)
    .join(" ");
}

/** 生成内联 SVG 字符串,供模板拼接与静态 HTML 校验共用。 */
export function iconSvg(name: UiIconName, className?: string): string {
  const cls = className ? ` class="${className}"` : "";
  const shapes = ICONS[name].map((s) => `<${s.tag} ${serializeAttrs(s.attrs)}/>`).join("");
  return `<svg${cls} ${serializeAttrs(SVG_BASE_ATTRS)}>${shapes}</svg>`;
}

/** 用安全 DOM API 创建 SVG 节点(兼容无 createElementNS 的精简测试 DOM)。 */
export function createIcon(name: UiIconName, className?: string): Element {
  const create =
    typeof document.createElementNS === "function"
      ? (tag: string) => document.createElementNS(SVG_NS, tag)
      : (tag: string) => document.createElement(tag);
  const svg = create("svg");
  if (className) svg.setAttribute("class", className);
  for (const [k, v] of Object.entries(SVG_BASE_ATTRS)) {
    svg.setAttribute(k, v);
  }
  for (const shape of ICONS[name]) {
    const node = create(shape.tag);
    for (const [k, v] of Object.entries(shape.attrs)) {
      node.setAttribute(k, v);
    }
    svg.appendChild(node);
  }
  return svg;
}
