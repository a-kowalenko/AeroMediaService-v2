/** SMB server session diagnostics + Safe-Close helpers (Phase 20c). */

import type { SmbSessionRow, SmbSessionSnapshot } from "@/lib/tauri";

export type {
  SmbSessionCloseDetail,
  SmbSessionCloseOutcome,
  SmbSessionCloseReport,
  SmbSessionQueryStatus,
  SmbSessionRow,
  SmbSessionSnapshot,
} from "@/lib/tauri";

export function smbSessionChipTitle(
  snapshot: SmbSessionSnapshot | null,
  atsClientCount: number,
): string {
  const base = "ATS-Clients anzeigen";
  if (!snapshot || snapshot.status === "unsupported") {
    return base;
  }
  if (snapshot.status === "permission_denied") {
    return `${base} — SMB-Sessions: Admin-Rechte nötig`;
  }
  if (snapshot.status === "error") {
    return `${base} — SMB-Sessions: Abfrage fehlgeschlagen`;
  }
  if (snapshot.warn) {
    return `${base} — ${snapshot.session_count} SMB-Sessions (Schwelle ${snapshot.warn_threshold}) · ATS: ${atsClientCount}`;
  }
  if (snapshot.session_count > 0) {
    return `${base} — ${snapshot.session_count} SMB-Sessions · ATS: ${atsClientCount}`;
  }
  return base;
}

export function formatSmbDuration(seconds: number): string {
  const s = Math.max(0, Math.floor(Number(seconds) || 0));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem > 0 ? `${h}h ${rem}m` : `${h}h`;
}

/** Safe-Close: Idle ≥ min and NumOpens == 0. */
export function isSmbSessionSafeToClose(
  row: SmbSessionRow,
  idleMinSeconds: number,
): boolean {
  const min = Math.max(60, Math.floor(Number(idleMinSeconds) || 600));
  return row.num_opens === 0 && row.seconds_idle >= min;
}

/** Bulk close candidates: Safe-Close + focus share when filter is known. */
export function isSmbSessionBulkCloseCandidate(
  row: SmbSessionRow,
  idleMinSeconds: number,
  focusShareKnown: boolean,
): boolean {
  if (!isSmbSessionSafeToClose(row, idleMinSeconds)) return false;
  if (focusShareKnown) return Boolean(row.on_focus_share);
  return true;
}

export function countSafeIdleSmbSessions(
  snapshot: SmbSessionSnapshot | null,
): number {
  if (!snapshot || snapshot.status !== "ok") return 0;
  const focusKnown = Boolean(snapshot.focus_share_name);
  return snapshot.sessions.filter((row) =>
    isSmbSessionBulkCloseCandidate(row, snapshot.idle_min_seconds, focusKnown),
  ).length;
}

export function closeSafeIdleSmbSessionsConfirmMessage(
  count: number,
  idleMinSeconds: number,
  focusShareName: string | null,
): string {
  const idleLabel = formatSmbDuration(idleMinSeconds);
  const focusLine = focusShareName
    ? `Nur Sessions auf Freigabe „${focusShareName}“ (Monitor/aktuell).`
    : "Share-Filter nicht auflösbar — Safe-Close für alle gelisteten Idle-Sessions.";
  return [
    `${count} SMB-Session(s) schließen?`,
    "",
    `Nur Sessions mit Idle ≥ ${idleLabel} und ohne offene Datei-Handles (Opens = 0).`,
    focusLine,
    "Aktive oder geöffnete Sessions bleiben unberührt.",
    "",
    "Abbruch ohne Änderung.",
  ].join("\n");
}

export function closeOneSmbSessionConfirmMessage(
  row: SmbSessionRow,
  idleMinSeconds: number,
): string {
  const idleLabel = formatSmbDuration(idleMinSeconds);
  const client = row.client_computer_name || "—";
  const user = row.client_user_name || "—";
  return [
    `SMB-Session von ${client} (${user}) schließen?`,
    "",
    `Nur erlaubt bei Idle ≥ ${idleLabel} und Opens = 0 (aktuell Idle ${formatSmbDuration(row.seconds_idle)}, Opens ${row.num_opens}).`,
    "",
    "Abbruch ohne Änderung.",
  ].join("\n");
}
