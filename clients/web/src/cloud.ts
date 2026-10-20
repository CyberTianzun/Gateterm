import { subscribe, t } from "./i18n";

export type CloudStatus =
  | { state: "connected"; email: string }
  | { state: "disconnected" | "loading" | "unavailable" };

export async function fetchCloudStatus(token: string, signal?: AbortSignal): Promise<CloudStatus> {
  const response = await fetch("/api/cloud/status", {
    headers: { Authorization: `Bearer ${token}` }, cache: "no-store", signal,
  });
  if (!response.ok) throw new Error("Cloud status unavailable");
  const body: unknown = await response.json();
  if (body && typeof body === "object" && "connected" in body) {
    if (body.connected === false) return { state: "disconnected" };
    if (body.connected === true && "email" in body && typeof body.email === "string" && body.email.trim()) {
      return { state: "connected", email: body.email };
    }
  }
  throw new Error("Invalid Cloud status");
}

export function cloudStatusText(status: CloudStatus): string {
  return status.state === "connected"
    ? t("cloud.connected", { email: status.email })
    : status.state === "disconnected" ? t("cloud.disconnected")
    : status.state === "loading" ? t("cloud.loading") : t("cloud.unavailable");
}

/** Poll only while connected and visible; returning from Desktop settings refreshes immediately. */
export function mountCloudStatus(element: HTMLElement, token: () => string) {
  let status: CloudStatus = { state: "loading" };
  let online = false;
  let pending: AbortController | null = null;
  let generation = 0;
  function render() {
    const text = cloudStatusText(status);
    if (element.textContent !== text) element.textContent = text;
    element.dataset.connected = String(status.state === "connected");
  }
  async function refresh() {
    if (!online || document.visibilityState === "hidden" || pending) return;
    const controller = new AbortController();
    const revision = generation;
    pending = controller;
    const timeout = setTimeout(() => controller.abort(), 8000);
    try {
      const next = await fetchCloudStatus(token(), controller.signal);
      if (revision === generation) { status = next; render(); }
    } catch {
      if (revision === generation) { status = { state: "unavailable" }; render(); }
    } finally {
      clearTimeout(timeout);
      if (pending === controller) pending = null;
    }
  }
  function setOnline(value: boolean) {
    generation++;
    pending?.abort();
    pending = null;
    online = value;
    if (value) void refresh();
    else { status = { state: "unavailable" }; render(); }
  }
  const onVisible = () => { void refresh(); };
  const timer = setInterval(onVisible, 10000);
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
