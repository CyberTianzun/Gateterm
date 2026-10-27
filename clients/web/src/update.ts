import { subscribe, t } from "./i18n";

export interface ReleaseUpdate { version: string; url: string }

/** Only the server checks GitHub; this request reads its cached daily result. */
export async function fetchUpdateStatus(token: string, signal?: AbortSignal): Promise<ReleaseUpdate | null> {
  const response = await fetch("/api/update", {
    headers: { Authorization: `Bearer ${token}` }, cache: "no-store", signal,
  });
  if (response.status === 404) return null; // Older servers have no update endpoint.
  if (!response.ok) throw new Error("Update status unavailable");
  const body = await response.json();
  if (body?.update === null) return null;
  const update = body?.update;
  if (!update || typeof update.version !== "string" || !update.version.trim() || update.version.length > 80 ||
      typeof update.url !== "string") throw new Error("Invalid update status");
  const url = new URL(update.url);
  if (url.protocol !== "https:" || url.hostname !== "github.com" || url.port || url.username || url.password ||
      !url.pathname.startsWith("/CyberTianzun/Gateterm/releases/")) throw new Error("Invalid Release URL");
  return { version: update.version, url: url.href };
}

export function mountUpdateStatus(element: HTMLElement, token: () => string) {
  let update: ReleaseUpdate | null = null;
  let online = false;
  let pending: AbortController | null = null;
  let generation = 0;
  function render() {
    element.hidden = !online || update === null;
    if (element.hidden || !update) { element.replaceChildren(); return; }
    const text = t("update.available", { version: update.version });
    const existing = element.querySelector<HTMLAnchorElement>("a");
    if (existing?.href === update.url && existing.textContent === text) return;
    const link = document.createElement("a");
    link.className = "update-link";
    link.href = update.url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = text;
    element.replaceChildren(link);
  }
  async function refresh() {
    if (!online || document.visibilityState === "hidden" || pending) return;
    const controller = new AbortController();
    const revision = generation;
    pending = controller;
    const timeout = setTimeout(() => controller.abort(), 8000);
    try {
      const next = await fetchUpdateStatus(token(), controller.signal);
      if (revision === generation) { update = next; render(); }
    } catch {
      // Temporary failures do not erase a previously validated update offer.
    } finally {
      clearTimeout(timeout);
      if (pending === controller) pending = null;
    }
  }
  function setOnline(value: boolean) {
    generation++;
    pending?.abort(); pending = null;
    online = value;
    if (!value) update = null;
    render();
    if (value) void refresh();
  }
  const onVisible = () => { void refresh(); };
  // Poll only the local in-memory cache, so new releases appear without a reload.
  const timer = setInterval(onVisible, 60_000);
  window.addEventListener("focus", onVisible);
  document.addEventListener("visibilitychange", onVisible);
  const unsubscribe = subscribe(render);
  render();
  return {
    setOnline,
    dispose() {
      online = false;
      generation++;
      pending?.abort();
      clearInterval(timer);
      unsubscribe();
      window.removeEventListener("focus", onVisible);
      document.removeEventListener("visibilitychange", onVisible);
    },
  };
}
