/** Prefix options for ATS path-hint SMB URL fields (parity with ATS server URL picker). */
export type SmbUrlScheme = "smb://" | "\\\\";

export const SMB_URL_SCHEME_OPTIONS: ReadonlyArray<{
  id: SmbUrlScheme;
  prefix: string;
}> = [
  { id: "smb://", prefix: "smb://" },
  { id: "\\\\", prefix: "\\\\" },
];

export function parseSmbUrlParts(url: string): {
  scheme: SmbUrlScheme;
  rest: string;
} {
  const raw = url.trim();
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
  return { scheme: "smb://", rest: raw };
}

export function composeSmbUrl(scheme: SmbUrlScheme, rest: string): string {
  const body = rest.trim();
  if (!body) return "";
  switch (scheme) {
    case "smb://":
      return `smb://${body.replace(/^\/+/, "")}`;
    case "\\\\":
      return `\\\\${body.replace(/^\\+/, "")}`;
  }
}

export function smbUrlSchemeLabel(scheme: SmbUrlScheme): string {
  return SMB_URL_SCHEME_OPTIONS.find((o) => o.id === scheme)?.prefix ?? scheme;
}

export function smbUrlRestPlaceholder(scheme: SmbUrlScheme): string {
  return scheme === "\\\\" ? "host\\aktuell" : "169.254.169.254/aktuell";
}
