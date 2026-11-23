/**
 * 把服务端 `clipboard` 控制消息里的文本写进本机剪贴板。
 *
 * 服务端只把它发给最近一次向会话输入过的那一端(拖选复制、`/copy` 都是这一端的
 * 操作触发的),所以这里不再做任何「该不该写」的判断。
 *
 * `navigator.clipboard` 只在安全上下文(https / localhost)里有,通过局域网 IP 用
 * http 打开时就没有;这时先退回无侵入的 `execCommand("copy")` + `copy` 事件(不移动焦点、
 * 不改选区,终端不会因此收到一对 focus out/in 上报);若浏览器因无选区拒绝,再退回
 * 离屏 `<textarea readonly>` 选区复制并恢复原焦点。两条路都依赖浏览器的「用户刚刚
 * 操作过」窗口 —— 复制紧跟在鼠标抬起/回车之后,通常在窗口内。
 */
function copyWithSelectionTextarea(text: string): boolean {
  if (typeof document.createElement !== "function" || !document.body?.appendChild) {
    return false;
  }
  const previous = document.activeElement as HTMLElement | null;
  const textarea = document.createElement("textarea");
  textarea.readOnly = true;
  textarea.setAttribute?.("readonly", "");
  textarea.value = text;
  if (textarea.style) {
    textarea.style.position = "fixed";
    textarea.style.left = "-9999px";
    textarea.style.top = "0";
    textarea.style.opacity = "0";
  }
  document.body.appendChild(textarea);
  try {
    textarea.focus?.({ preventScroll: true });
    textarea.select?.();
    textarea.setSelectionRange?.(0, text.length);
    return Boolean(document.execCommand("copy"));
  } catch {
    return false;
  } finally {
    if (typeof textarea.remove === "function") {
      textarea.remove();
    } else {
      textarea.parentNode?.removeChild(textarea);
    }
    try {
      previous?.focus?.({ preventScroll: true });
    } catch {
      // 忽略恢复焦点时的异常
    }
  }
}

export async function writeClipboard(text: string): Promise<boolean> {
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // 页面没聚焦 / 权限被拒:再试一次老办法
    }
  }
  let done = false;
  const onCopy = (ev: ClipboardEvent) => {
    if (!ev.clipboardData) return;
    ev.clipboardData.setData("text/plain", text);
    ev.preventDefault();
    done = true;
  };
  document.addEventListener("copy", onCopy, true);
  try {
    document.execCommand("copy");
  } catch {
    // 不支持就算了
  } finally {
    document.removeEventListener("copy", onCopy, true);
  }
  if (!done) {
    done = copyWithSelectionTextarea(text);
  }
  if (!done) console.warn("gateterm: clipboard write was blocked by the browser");
  return done;
}
