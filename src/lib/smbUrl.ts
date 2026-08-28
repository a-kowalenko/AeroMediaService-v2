/** Prefix options for ATS path-hint fields (parity with ATS server URL picker + local path). */
export type SmbUrlScheme = "smb://" | "\\\\" | "local";

export const SMB_URL_SCHEME_OPTIONS: ReadonlyArray<{
  id: SmbUrlScheme;
  prefix: string;
}> = [
  { id: "smb://", prefix: "smb://" },
  { id: "\\\\", prefix: "\\\\" },
  { id: "local", prefix: "Pfad" },
];

/** Whether `raw` looks like an absolute local filesystem path (not SMB/UNC). */
export function isLocalPath(raw: string): boolean {
  const t = raw.trim();
  if (!t) return false;
  if (t.length >= 6 && t.slice(0, 6).toLowerCase() === "smb://") return false;
  if (t.startsWith("\\\\") || t.startsWith("//")) return false;
  if (/^[A-Za-z]:([/\\]|$)/.test(t)) return true;
  return t.startsWith("/");
}

/** Strip invalid `smb://` prefix from local paths (e.g. `smb://C:\\Share` → `C:\\Share`). */
export function sanitizeSharePath(raw: string): string {
  const t = raw.trim();
  if (!t) return "";
  if (t.length >= 6 && t.slice(0, 6).toLowerCase() === "smb://") {
    const rest = t.slice(6).replace(/^\/+/, "");
    if (isLocalPath(rest)) return rest;
    return `smb://${rest.replace(/\\/g, "/")}`;
  }
  if (t.startsWith("\\\\") || t.startsWith("//")) {
    const rest = t.startsWith("\\\\") ? t.slice(2) : t.slice(2);
    return `smb://${rest.replace(/\\/g, "/")}`;
  }
  return t;
}

/** True when `raw` is a client-tauglicher SMB/UNC URL (not a plain local path). */
export function isNetworkShareUrl(raw: string): boolean {
  const s = sanitizeSharePath(raw);
  if (!s) return false;
  return s.length >= 6 && s.slice(0, 6).toLowerCase() === "smb://";
}

export function parseSmbUrlParts(url: string): {
  scheme: SmbUrlScheme;
  rest: string;
} {
  const raw = sanitizeSharePath(url.trim());
  if (!raw) return { scheme: "smb://", rest: "" };
  const lower = raw.toLowerCase();
  if (lower.startsWith("smb://")) {
    return { scheme: "smb://", rest: raw.slice(6) };
  }
  if (raw.startsWith("\\\\")) {
    return { scheme: "\\\\", rest: raw.slice(2) };
  }
  // Treat POSIX-style //host/share as UNC in the UI.
  if (raw.startsWith("//")) {
    return { scheme: "\\\\", rest: raw.slice(2) };
  }
  if (isLocalPath(raw)) {
    return { scheme: "local", rest: raw };
  }
  return { scheme: "smb://", rest: raw };
}

export function composeSmbUrl(scheme: SmbUrlScheme, rest: string): string {
  const body = rest.trim();
  if (!body) return "";
  if (scheme === "smb://" && isLocalPath(body)) {
    return body;
  }
  switch (scheme) {
    case "smb://":
      return `smb://${body.replace(/^\/+/, "")}`;
    case "\\\\":
      return `\\\\${body.replace(/^\\+/, "")}`;
    case "local":
      return body;
  }
}

export function smbUrlSchemeLabel(scheme: SmbUrlScheme): string {
  if (scheme === "local") return "Pfad";
  return SMB_URL_SCHEME_OPTIONS.find((o) => o.id === scheme)?.prefix ?? scheme;
}

export function smbUrlRestPlaceholder(scheme: SmbUrlScheme): string {
  switch (scheme) {
    case "\\\\":
      return "host\\aktuell";
    case "local":
      return "D:\\Shares\\aktuell";
    default:
      return "169.254.169.254/aktuell";
  }
}
