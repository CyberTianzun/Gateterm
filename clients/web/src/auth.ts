/** 读取分享链接里的 token 后替换当前历史记录，避免凭据一直留在地址栏。 */
export function readToken(promptMessage: string): string {
  const params = new URLSearchParams(location.hash.slice(1));
  const fromHash = params.get("token");
  if (fromHash) localStorage.setItem("octoterm-token", fromHash);
  if (params.has("token")) {
    params.delete("token");
    const hash = params.toString();
    history.replaceState(history.state, "", `${location.pathname}${location.search}${hash ? `#${hash}` : ""}`);
  }
  return localStorage.getItem("octoterm-token") ?? prompt(promptMessage) ?? "";
}
