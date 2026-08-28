import type { ComboboxOption } from "@/components/ui/combobox";
import type { LocalShareCandidate } from "@/lib/tauri";
import { isNetworkShareUrl, sanitizeSharePath } from "@/lib/smbUrl";

function normPath(raw: string): string {
  const s = sanitizeSharePath(raw);
  if (!s) return "";
  let key = s.toLowerCase().replace(/\\/g, "/");
  while (key.endsWith("/") && key.length > 1) {
    key = key.slice(0, -1);
  }
  return key;
}

/** Case-insensitive path key for compare/dedupe (exported for Settings draft logic). */
export function sharePathKey(raw: string): string {
  return normPath(raw);
}

/** Map backend candidates to combobox rows (de-duplicated, paths sanitized). */
export function toShareComboboxOptions(
  candidates: readonly LocalShareCandidate[],
  excludePath?: string,
): ComboboxOption[] {
  const exclude = excludePath ? normPath(excludePath) : "";
  const seen = new Set<string>();
  const out: ComboboxOption[] = [];
  for (const c of candidates) {
    const path = sanitizeSharePath(c.path);
    if (!path) continue;
    const key = normPath(path);
    if (!key || seen.has(key) || (exclude && key === exclude)) continue;
    seen.add(key);
    out.push({ value: path, label: c.label });
  }
  return out;
}

/** Add unsaved draft paths when they are not already listed (e.g. after editing before save). */
export function appendDraftShareOptions(
  options: readonly ComboboxOption[],
  drafts: ReadonlyArray<{ path: string; label: string }>,
): ComboboxOption[] {
  const out = [...options];
  const seen = new Set(options.map((o) => normPath(o.value)));
  for (const { path, label } of drafts) {
    const sanitized = sanitizeSharePath(path);
    const key = normPath(sanitized);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push({ value: sanitized, label });
  }
  return out;
}

/** Suggest a backup SMB URL from primary (network shares only). */
export function deriveBackupShareSuggestion(primaryRaw: string): string | null {
  const primary = sanitizeSharePath(primaryRaw.trim());
  if (!primary || !isNetworkShareUrl(primary)) return null;
  const suffix = "-backup";
  if (primary.toLowerCase().endsWith(suffix)) return null;
  const trimmed = primary.replace(/\/+$/, "");
  return `${trimmed}${suffix}`;
}
