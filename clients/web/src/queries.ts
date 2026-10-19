/**
 * 终端查询(问光标位置、问颜色、问终端身份……)只由服务端应答,浏览器端一律不答。
 *
 * xterm.js 默认会通过 onData 回答这些查询,而 onData 直接发往 pty。应答要绕
 * 「服务端 → 网络 → 浏览器 → 网络 → 服务端」一圈,发问的程序常常已经超时退出,
 * 迟到的应答就落进 shell,成了一行 `11;rgb:1919/1a1a/1b1b2;1R` 这样的乱码。
 * 服务端那份 alacritty Term 本来就在应答(crates/server/src/session/grid.rs),
 * 浏览器再答一遍只是重复;重连重放环形缓冲时,旧的查询还会被再答一次。
 *
 * 颜色查询服务端得知道本端用什么颜色画,所以 attach/resize 时带上 termColors()。
 */
import type { ITheme, Terminal } from "@xterm/xterm";

export interface TermColors {
  fg: string;
  bg: string;
}

/** 返回 true 表示已处理,xterm 不再走它自己的应答逻辑。 */
const swallow = () => true;

export function swallowTerminalQueries(term: Terminal) {
  const { parser } = term;
  for (const prefix of [undefined, ">", "="]) parser.registerCsiHandler({ prefix, final: "c" }, swallow); // DA1/2/3
  for (const prefix of [undefined, "?"]) {
    parser.registerCsiHandler({ prefix, final: "n" }, swallow); // DSR / DECXCPR
    parser.registerCsiHandler({ prefix, intermediates: "$", final: "p" }, swallow); // DECRQM
  }
  parser.registerCsiHandler({ prefix: ">", final: "q" }, swallow); // XTVERSION
  parser.registerDcsHandler({ intermediates: "$", final: "q" }, swallow); // DECRQSS
  // 颜色:只拦查询(含 `?`),设色照常交给 xterm 去画。
  for (const ident of [4, 10, 11, 12]) {
    parser.registerOscHandler(ident, (data) => data.includes("?"));
  }
}

/** xterm 主题缺省时的前景/背景,和 xterm.js 自己的默认一致。 */
const XTERM_FG = "#ffffff";
const XTERM_BG = "#000000";

/** 主题颜色规整成服务端要的 `#rrggbb`;认不出来的颜色退回 xterm 的默认值。 */
export function termColors(theme: ITheme | undefined): TermColors {
  return {
    fg: toRrggbb(theme?.foreground) ?? XTERM_FG,
    bg: toRrggbb(theme?.background) ?? XTERM_BG,
  };
}

function toRrggbb(color: string | undefined): string | null {
  if (!color || !/^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(color)) return null;
  const h = color.slice(1).toLowerCase();
  if (h.length <= 4) return "#" + [...h.slice(0, 3)].map((c) => c + c).join("");
  return "#" + h.slice(0, 6);
}
