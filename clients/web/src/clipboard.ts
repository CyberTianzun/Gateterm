/**
 * 把服务端 `clipboard` 控制消息里的文本写进本机剪贴板。
 *
 * 服务端只把它发给最近一次向会话输入过的那一端(拖选复制、`/copy` 都是这一端的
 * 操作触发的),所以这里不再做任何「该不该写」的判断。
 *
 * `navigator.clipboard` 只在安全上下文(https / localhost)里有,通过局域网 IP 用
 * http 打开时就没有;这时退回 `execCommand("copy")` + `copy` 事件:不移动焦点、不
 * 改选区,终端不会因此收到一对 focus out/in 上报。两条路都依赖浏览器的「用户刚刚
 * 操作过」窗口 —— 复制紧跟在鼠标抬起/回车之后,通常在窗口内。
 */
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
  if (!done) console.warn("gateterm: clipboard write was blocked by the browser");
  return done;
}
