/**
 * Pure-frontend pairing URL validation and SVG QR code generation.
 *
 * Validation rules mirror Android's `ConnectionQr.kt` and `ConnectionUrl.kt`
 * so any QR code rendered here is accepted by the mobile scanner without
 * surprises.
 */
import qrcode from "qrcode-generator";

export type PairingErrorReason =
  | "missing-token"
  | "unsupported-scheme"
  | "userinfo-not-allowed"
  | "loopback"
  | "invalid-url";

export type PairingTarget =
  | { ok: true; url: string; displayUrl: string }
  | { ok: false; reason: PairingErrorReason; host?: string };

export type QrMatrixResult =
  | {
      ok: true;
      moduleCount: number;
      quietZone: number;
      viewBoxSize: number;
      pathData: string;
    }
  | { ok: false; reason: "too-long" };

const SVG_NS = "http://www.w3.org/2000/svg";
const QUIET_ZONE = 4;
const MAX_QR_TEXT_LENGTH = 8192;

// qrcode-generator@2.0.4 defaults `stringToBytes` to `c & 0xff` (Latin-1 truncation);
// override with UTF-8 bytes so multi-byte URLs/tokens encode faithfully.
const utf8Encoder = new TextEncoder();
qrcode.stringToBytes = (s: string): number[] => Array.from(utf8Encoder.encode(s));

function isLoopbackOrWildcardHost(host: string): boolean {
  return (
    host === "localhost" ||
    host === "::1" ||
    host === "0.0.0.0" ||
    host === "::" ||
    host.startsWith("127.") ||
    host.startsWith("::ffff:127.") ||
    // WHATWG URL normalizes `[::ffff:127.x.y.z]` to `[::ffff:7fxx:yyzz]`.
    /^::ffff:7f[0-9a-f]{2}:[0-9a-f]{1,4}$/.test(host)
  );
}

/**
 * Validate the current page `href` and active `token`, returning a pairing URL
 * (`<base>#token=...` or `<base>#token=...&title=...` when a non-empty `title`
 * is provided) suitable for QR encoding or a structured rejection reason.
 */
export function buildPairingTarget(
  href: string,
  token: string,
  title?: string | null,
): PairingTarget {
  let parsed: URL;
  try {
    parsed = new URL(href);
  } catch {
    return { ok: false, reason: "invalid-url" };
  }

  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { ok: false, reason: "unsupported-scheme" };
  }

  if (parsed.username !== "" || parsed.password !== "") {
    return { ok: false, reason: "userinfo-not-allowed" };
  }

  if (parsed.port === "0") {
    return { ok: false, reason: "invalid-url" };
  }

  const host = parsed.hostname.replace(/^\[|\]$/g, "").toLowerCase();
  if (!host) {
    return { ok: false, reason: "invalid-url" };
  }

  if (isLoopbackOrWildcardHost(host)) {
    return { ok: false, reason: "loopback", host };
  }

  if (!token.trim()) {
    return { ok: false, reason: "missing-token" };
  }

  const base = new URL(".", parsed).toString();
  const displayUrl = base.replace(/\/$/, "");
  const params = new URLSearchParams([["token", token]]);
  const normalizedTitle =
    typeof title === "string" ? title.replace(/[\r\n]+/g, " ").trim() : "";
  if (normalizedTitle) {
    params.append("title", normalizedTitle);
  }
  return {
    ok: true,
    url: `${base}#${params.toString()}`,
    displayUrl,
  };
}

/**
 * Encode `text` into an EC-M QR code and build a 4-module-padded SVG path model.
 */
export function buildQrSvgModel(text: string): QrMatrixResult {
  if (text.length > MAX_QR_TEXT_LENGTH) {
    return { ok: false, reason: "too-long" };
  }

  try {
    const qr = qrcode(0, "M");
    qr.addData(text, "Byte");
    qr.make();

    const moduleCount = qr.getModuleCount();
    const viewBoxSize = moduleCount + QUIET_ZONE * 2;
    const segments: string[] = [];

    for (let r = 0; r < moduleCount; r += 1) {
      for (let c = 0; c < moduleCount; c += 1) {
        if (qr.isDark(r, c)) {
          segments.push(`M${c + QUIET_ZONE} ${r + QUIET_ZONE}h1v1h-1z`);
        }
      }
    }

    return {
      ok: true,
      moduleCount,
      quietZone: QUIET_ZONE,
      viewBoxSize,
      pathData: segments.join(""),
    };
  } catch {
    return { ok: false, reason: "too-long" };
  }
}

/**
 * Render `text` to a self-contained SVG string with a 4-module white quiet zone.
 */
export function renderQrSvg(
  text: string,
): { ok: true; svg: string } | { ok: false; reason: "too-long" } {
  const model = buildQrSvgModel(text);
  if (!model.ok) {
    return model;
  }

  const size = String(model.viewBoxSize);
  return {
    ok: true,
    svg: `<svg xmlns="${SVG_NS}" viewBox="0 0 ${size} ${size}" role="img" shape-rendering="crispEdges"><rect width="${size}" height="${size}" fill="#ffffff"/><path d="${model.pathData}" fill="#000000"/></svg>`,
  };
}

/**
 * Construct an accessible `<svg>` element via `document.createElementNS` (never `innerHTML`).
 */
export function createQrSvgElement(
  model: Extract<QrMatrixResult, { ok: true }>,
  label: string,
): SVGSVGElement {
  const size = String(model.viewBoxSize);
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${size} ${size}`);
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", label);
  svg.setAttribute("shape-rendering", "crispEdges");

  const bg = document.createElementNS(SVG_NS, "rect");
  bg.setAttribute("width", size);
  bg.setAttribute("height", size);
  bg.setAttribute("fill", "#ffffff");

  const path = document.createElementNS(SVG_NS, "path");
  path.setAttribute("d", model.pathData);
  path.setAttribute("fill", "#000000");

  svg.append(bg, path);
  return svg;
}
