import {useEffect, useMemo, useState} from "react";
import {
  closeSafeIdleSmbSessionsConfirmMessage,
  closeOneSmbSessionConfirmMessage,
  countSafeIdleSmbSessions,
  formatSmbDuration,
  isSmbSessionSafeToClose,
  type SmbSessionSnapshot,
} from "@/lib/smbSessions";
import {
  closeSafeIdleSmbSessions,
  closeSafeIdleSmbSessionsElevated,
  closeSmbSession,
  closeSmbSessionElevated,
  getSmbSessionSnapshotElevated,
  type SmbSessionCloseReport,
  type SmbSessionRow,
} from "@/lib/tauri";
import {Button} from "@/components/ui/button";
import {useUiStore} from "@/store/uiStore";

type Props = {
  snapshot: SmbSessionSnapshot | null;
  loading?: boolean;
  /** When false, clear dialog-local elevated cache (dialog closed). */
  active?: boolean;
  onChanged?: () => void | Promise<void>;
};

export function SmbSessionsSection({
  snapshot,
  loading,
  active = true,
  onChanged,
}: Props) {
  const confirm = useUiStore((s) => s.confirm);
  const showSuccess = useUiStore((s) => s.showSuccess);
  const showWarning = useUiStore((s) => s.showWarning);
  const showError = useUiStore((s) => s.showError);
  const [busy, setBusy] = useState(false);
  /** Elevated snapshot for dialog lifetime only — never auto-re-elevate on poll. */
  const [elevatedSnapshot, setElevatedSnapshot] = useState<SmbSessionSnapshot | null>(null);

  useEffect(() => {
    if (!active) {
      setElevatedSnapshot(null);
    }
  }, [active]);

  const view = elevatedSnapshot ?? snapshot;
  const usingElevated = Boolean(elevatedSnapshot);
  const safeCount = useMemo(() => countSafeIdleSmbSessions(view), [view]);

  async function applyCloseReport(report: SmbSessionCloseReport) {
    if (usingElevated) {
      try {
        const next = await getSmbSessionSnapshotElevated();
        if (next.status === "ok") {
          setElevatedSnapshot(next);
        }
      } catch {
        // keep previous elevated view
      }
    }
    await onChanged?.();
    if (report.message.includes("abgebrochen") || report.message.includes("UAC")) {
      showWarning(report.message, "UAC abgebrochen");
      return;
    }
    if (report.permission_denied) {
      showWarning(report.message, "Admin-Rechte nötig");
      return;
    }
    if (report.failed > 0 && report.closed === 0) {
      showError(report.message, "SMB-Session schließen");
      return;
    }
    if (report.closed > 0) {
      showSuccess(report.message, "SMB-Sessions");
      return;
    }
    showWarning(report.message, "SMB-Sessions");
  }

  async function onLoadElevated() {
    if (busy) return;
    setBusy(true);
    try {
      const next = await getSmbSessionSnapshotElevated();
      if (next.status === "ok") {
        setElevatedSnapshot(next);
        showSuccess(
          "SMB-Sessions mit Admin-Rechten geladen. AMS läuft weiter normal.",
          "SMB-Sessions",
        );
        await onChanged?.();
        return;
      }
      if (next.message.includes("abgebrochen") || next.message.includes("UAC")) {
        showWarning(next.message, "UAC abgebrochen");
        return;
      }
      showWarning(next.message, "Admin-Rechte nötig");
    } catch (err) {
      showError(
        err instanceof Error ? err.message : String(err),
        "SMB-Sessions laden",
      );
    } finally {
      setBusy(false);
    }
  }

  async function onCloseAllSafe() {
    if (!view || view.status !== "ok" || safeCount === 0 || busy) return;
    const ok = await confirm(
      closeSafeIdleSmbSessionsConfirmMessage(
        safeCount,
        view.idle_min_seconds,
        view.focus_share_name,
      ),
      {
        title: usingElevated
          ? "Sichere Idle-Sessions schließen (Admin)"
          : "Sichere Idle-Sessions schließen",
        primaryLabel: usingElevated ? "Mit Admin schließen" : "Schließen",
        secondaryLabel: "Abbrechen",
        destructive: true,
      },
    );
    if (!ok) return;
    setBusy(true);
    try {
      const report = usingElevated
        ? await closeSafeIdleSmbSessionsElevated()
        : await closeSafeIdleSmbSessions();
      await applyCloseReport(report);
    } catch (err) {
      showError(
        err instanceof Error ? err.message : String(err),
        "SMB-Session schließen",
      );
    } finally {
      setBusy(false);
    }
  }

  async function onCloseOne(row: SmbSessionRow) {
    if (!view || view.status !== "ok" || busy) return;
    if (!isSmbSessionSafeToClose(row, view.idle_min_seconds)) return;
    const ok = await confirm(
      closeOneSmbSessionConfirmMessage(row, view.idle_min_seconds),
      {
        title: usingElevated ? "SMB-Session schließen (Admin)" : "SMB-Session schließen",
        primaryLabel: usingElevated ? "Mit Admin schließen" : "Schließen",
        secondaryLabel: "Abbrechen",
        destructive: true,
      },
    );
    if (!ok) return;
    setBusy(true);
    try {
      const report = usingElevated
        ? await closeSmbSessionElevated(row.session_id)
        : await closeSmbSession(row.session_id);
      await applyCloseReport(report);
    } catch (err) {
      showError(
        err instanceof Error ? err.message : String(err),
        "SMB-Session schließen",
      );
    } finally {
      setBusy(false);
    }
  }

  async function onCloseElevatedDenied() {
    if (busy) return;
    const ok = await confirm(
      [
        "SMB-Sessions mit Administratorrechten schließen?",
        "",
        "AMS bleibt normal (ohne Admin) geöffnet.",
        "Es erscheint einmal die Windows-Benutzerkontensteuerung (UAC).",
        "",
        "Abbruch ohne Änderung.",
      ].join("\n"),
      {
        title: "Mit Admin-Rechten schließen",
        primaryLabel: "UAC fortsetzen",
        secondaryLabel: "Abbrechen",
        destructive: true,
      },
    );
    if (!ok) return;
    setBusy(true);
    try {
      const report = await closeSafeIdleSmbSessionsElevated();
      if (report.permission_denied || report.message.includes("abgebrochen")) {
        await applyCloseReport(report);
        return;
      }
      // Also refresh elevated list after close attempt.
      try {
        const next = await getSmbSessionSnapshotElevated();
        if (next.status === "ok") setElevatedSnapshot(next);
      } catch {
        // ignore
      }
      await applyCloseReport(report);
    } catch (err) {
      showError(
        err instanceof Error ? err.message : String(err),
        "SMB-Session schließen",
      );
    } finally {
      setBusy(false);
    }
  }

  if (!view) {
    return (
      <section className="space-y-2 rounded-lg border border-border/60 bg-muted/10 p-3">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">
          SMB-Sessions
        </h3>
        <p className="text-sm text-muted">
          {loading ? "SMB-Sessions werden geladen…" : "Noch keine SMB-Diagnose verfügbar."}
        </p>
      </section>
    );
  }

  if (view.status === "unsupported") {
    return (
      <section className="space-y-2 rounded-lg border border-border/60 bg-muted/10 p-3">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">
          SMB-Sessions
        </h3>
        <p className="text-sm text-muted">{view.message}</p>
      </section>
    );
  }

  const warnTone =
    view.warn || view.status === "permission_denied"
      ? "border-warning/45 bg-warning/10"
      : "border-border/60 bg-muted/10";
  const idleLabel = formatSmbDuration(view.idle_min_seconds);
  const canClose = view.status === "ok" && safeCount > 0 && !busy;
  const focusLabel = view.focus_share_name
    ? `Fokus: „${view.focus_share_name}“`
    : "Fokus: —";
  const isWindows = view.platform === "windows";
  const showElevateCta =
    isWindows &&
    !usingElevated &&
    (view.status === "permission_denied" ||
      (view.status === "ok" && !view.focus_share_name && view.session_count > 0));

  return (
    <section className={`space-y-3 rounded-lg border p-3 ${warnTone}`}>
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">
            SMB-Sessions
          </h3>
          <p className="mt-1 text-sm text-foreground">{view.message}</p>
          <p className="mt-1 text-xs text-muted">
            Warnschwelle: {view.warn_threshold} · Idle-Min: {idleLabel} · Poll:{" "}
            {view.poll_seconds}s · {focusLabel}
            {view.auto_close_enabled ? " · Auto-Close an" : ""}
            {usingElevated ? " · Admin-Helper (Dialog)" : ""}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {view.warn ? (
            <span className="inline-flex items-center rounded border border-warning/45 bg-warning/10 px-1.5 py-0.5 text-[10px] font-medium text-warning">
              Überlast / Hinweis
            </span>
          ) : null}
          {view.status === "ok" ? (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={!canClose}
              onClick={() => void onCloseAllSafe()}
              title={
                safeCount === 0
                  ? `Keine Kandidaten (Idle ≥ ${idleLabel}, Opens = 0${
                      view.focus_share_name
                        ? `, Fokus „${view.focus_share_name}“`
                        : ""
                    })`
                  : `${safeCount} sichere Idle-Session(s) schließen`
              }
            >
              {usingElevated ? "Idle mit Admin schließen…" : "Idle schließen…"}
            </Button>
          ) : null}
        </div>
      </div>

      {view.status === "permission_denied" ? (
        <div className="space-y-2">
          <p className="text-xs text-warning">
            Lesen/Schließen der SMB-Server-Sessions erfordert oft Administratorrechte. AMS bleibt
            normal geöffnet — einmalige UAC-Zustimmung nur für den SMB-Helper.
          </p>
          {isWindows ? (
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                size="sm"
                disabled={busy}
                onClick={() => void onLoadElevated()}
              >
                Mit Admin-Rechten laden…
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                disabled={busy}
                onClick={() => void onCloseElevatedDenied()}
              >
                Mit Admin-Rechten schließen…
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}

      {showElevateCta && view.status === "ok" ? (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            Share-Fokus nicht auflösbar (oft fehlende Rechte). Optional mit Admin-Rechten neu laden.
          </p>
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => void onLoadElevated()}
          >
            Mit Admin-Rechten laden…
          </Button>
        </div>
      ) : null}

      {view.status === "error" ? (
        <div className="space-y-2">
          <p className="text-xs text-destructive">{view.message}</p>
          {isWindows ? (
            <Button
              type="button"
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => void onLoadElevated()}
            >
              Mit Admin-Rechten laden…
            </Button>
          ) : null}
        </div>
      ) : null}

      {view.sessions.length === 0 ? (
        view.status === "ok" ? (
          <p className="text-sm text-muted">Aktuell keine offenen SMB-Server-Sessions.</p>
        ) : null
      ) : (
        <div className="max-h-48 overflow-auto rounded-md border border-border/50 bg-background/80">
          <table className="w-full min-w-[42rem] border-collapse text-left text-xs">
            <thead className="border-b border-border/60 bg-muted/20 text-[10px] uppercase tracking-wide text-muted">
              <tr>
                <th className="px-2 py-1.5 font-medium">Client</th>
                <th className="px-2 py-1.5 font-medium">Benutzer</th>
                <th className="px-2 py-1.5 font-medium">Share</th>
                <th className="px-2 py-1.5 font-medium">Bridge</th>
                <th className="px-2 py-1.5 font-medium">Idle</th>
                <th className="px-2 py-1.5 font-medium">Besteht</th>
                <th className="px-2 py-1.5 font-medium">Opens</th>
                <th className="px-2 py-1.5 font-medium">Aktion</th>
              </tr>
            </thead>
            <tbody>
              {view.sessions.map((row) => {
                const safe = isSmbSessionSafeToClose(row, view.idle_min_seconds);
                return (
                  <tr
                    key={row.session_id}
                    className="border-b border-border/40 last:border-b-0"
                  >
                    <td className="px-2 py-1.5 font-medium text-foreground">
                      {row.client_computer_name || "—"}
                    </td>
                    <td className="px-2 py-1.5 text-muted">{row.client_user_name || "—"}</td>
                    <td className="px-2 py-1.5">
                      {row.on_focus_share ? (
                        <span className="inline-flex items-center rounded border border-primary/35 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                          {view.focus_share_name || "Fokus"}
                        </span>
                      ) : view.focus_share_name ? (
                        <span className="text-[10px] text-muted">andere</span>
                      ) : (
                        <span className="text-[10px] text-muted">—</span>
                      )}
                    </td>
                    <td className="px-2 py-1.5">
                      {row.ats_hint ? (
                        <span
                          className="inline-flex items-center rounded border border-border/60 bg-muted/30 px-1.5 py-0.5 text-[10px] font-medium text-foreground"
                          title="ATS-Presence-Hinweis (kein Kill-Gate)"
                        >
                          ATS? {row.ats_hint}
                        </span>
                      ) : (
                        <span className="text-[10px] text-muted">—</span>
                      )}
                    </td>
                    <td className="px-2 py-1.5 tabular-nums text-muted">
                      {formatSmbDuration(row.seconds_idle)}
                    </td>
                    <td className="px-2 py-1.5 tabular-nums text-muted">
                      {formatSmbDuration(row.seconds_exists)}
                    </td>
                    <td className="px-2 py-1.5 tabular-nums text-muted">{row.num_opens}</td>
                    <td className="px-2 py-1.5">
                      {safe ? (
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          className="h-7 px-2 text-xs"
                          disabled={busy}
                          onClick={() => void onCloseOne(row)}
                        >
                          {usingElevated ? "Admin-Schließen" : "Schließen"}
                        </Button>
                      ) : (
                        <span className="text-[10px] text-muted">aktiv / Opens</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <p className="text-[11px] text-muted">
        Safe-Close nur bei Idle ≥ {idleLabel} und Opens = 0
        {view.focus_share_name
          ? `; Bulk/Auto nur auf Freigabe „${view.focus_share_name}“`
          : ""}
        . Auto-Close eleviert nicht (kein UAC-Spam). OS-Idle-Timeout und Server-Limits sind
        nachhaltiger als App-Kill — dies ist ein Notnagel (Soft-Policy).
      </p>
    </section>
  );
}
