/** 读取分享链接里的 token 后替换当前历史记录，避免凭据一直留在地址栏。 */
export function readToken(promptMessage: string): string {
  const params = new URLSearchParams(location.hash.slice(1));
  const fromHash = params.get("token");
  if (params.has("token")) {
    params.delete("token");
    const hash = params.toString();
    history.replaceState(history.state, "", `${location.pathname}${location.search}${hash ? `#${hash}` : ""}`);
  }
  let saved: string | null = null;
  try { saved = localStorage.getItem("gateterm-token"); } catch { /* Storage may be unavailable. */ }
  const token = fromHash || saved || (prompt(promptMessage) ?? "").trim();
  if (token) {
    try { localStorage.setItem("gateterm-token", token); } catch { /* The caller retains it for this page. */ }
  }
  return token;
}
