import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ChevronLeft,
  ChevronRight,
  MoreHorizontal,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import { ResendNotificationsDialog } from "./ResendNotificationsDialog";
import { StatusDot } from "./StatusLight";
import { VirtualList } from "./VirtualList";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { UPLOAD_HISTORY_UPDATE } from "@/lib/events";
import {
  canResendNotifications,
  canRetryUpload,
  cn,
  extraNumber,
  formatHistoryDate,
  formatManualStatusSummary,
  formatResendHistorySummary,
  historyDisplayName,
  overallStatusColor,
} from "@/lib/utils";
import type { HistoryEntry } from "@/lib/tauri";
import {
  channelsDelivered,
  getManualStatusWarnings,
  getSandboxWarnings,
  resendHistoryNotifications,
  retryUpload,
  saveHistoryContact,
  setManualStatus,
  syncSmsJournal,
} from "@/lib/tauri";
import { isCloudConnected, useAppStore } from "@/store/appStore";
import { useHistoryStore } from "@/store/historyStore";

const MANUAL_ACTIONS: Array<[string, string]> = [
  ["Komplett", "Als Komplett markieren"],
  ["Versendet", "Als Versendet markieren"],
  ["Problem auflösen", "Problem auflösen"],
];

const DETAIL_ROWS: Array<[string, (item: HistoryEntry) => string]> = [
  ["Verzeichnis", (i) => i.dir_name || "—"],
  ["Upload-Status", (i) => i.status || "—"],
  ["E-Mail-Status", (i) => i.email_status || "—"],
  ["SMS-Status", (i) => i.sms_status || "—"],
  ["Gesamtstatus", (i) => i.overall_status || "—"],
  ["E-Mail", (i) => i.email || "—"],
  ["Telefon", (i) => i.phone || "—"],
  ["Download-Link", (i) => i.share_link || "—"],
  ["Archiv", (i) => i.archived_path || "—"],
  ["Fehlertext", (i) => i.combined_error || "—"],
  ["Wiederversand", (i) => formatResendHistorySummary(i)],
  ["Manueller Status", (i) => formatManualStatusSummary(i)],
  [
    "Retry",
    (i) => {
      const n = extraNumber(i, "retry_count");
      return n ? `${n}×` : "—";
    },
  ],
];

export function HistoryTable() {
  const items = useHistoryStore((s) => s.items);
  const total = useHistoryStore((s) => s.total);
  const page = useHistoryStore((s) => s.page);
  const pageSize = useHistoryStore((s) => s.pageSize);
  const search = useHistoryStore((s) => s.search);
  const selectedId = useHistoryStore((s) => s.selectedId);
  const loading = useHistoryStore((s) => s.loading);
  const error = useHistoryStore((s) => s.error);
  const setSearch = useHistoryStore((s) => s.setSearch);
  const setPage = useHistoryStore((s) => s.setPage);
  const select = useHistoryStore((s) => s.select);
  const load = useHistoryStore((s) => s.load);
  const removeSelected = useHistoryStore((s) => s.removeSelected);
  const removeAll = useHistoryStore((s) => s.removeAll);
  const connectionStatus = useAppStore((s) => s.connectionStatus);
  const connected = isCloudConnected(connectionStatus);

  const [contactEmail, setContactEmail] = useState("");
  const [contactPhone, setContactPhone] = useState("");
  const [contactDirty, setContactDirty] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionMessage, setActionMessage] = useState("");
  const [actionError, setActionError] = useState("");
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [resendOpen, setResendOpen] = useState(false);
  const [sandboxWarnings, setSandboxWarnings] = useState<string[]>([]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: (() => void) | undefined;
    listen(UPLOAD_HISTORY_UPDATE, () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        void useHistoryStore.getState().load({ maintainPage: true });
      }, 400);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      if (timer) clearTimeout(timer);
      unlisten?.();
    };
  }, []);

  const selected = useMemo(
    () => items.find((item) => item.id === selectedId) ?? null,
    [items, selectedId],
  );

  useEffect(() => {
    setContactEmail(selected?.email ?? "");
    setContactPhone(selected?.phone ?? "");
    setContactDirty(false);
    setStatusMenuOpen(false);
  }, [selected?.id, selected?.email, selected?.phone]);

  const maxPage = Math.max(0, Math.ceil(total / pageSize) - 1 || 0);
  const pageCount = Math.max(1, Math.ceil(total / pageSize) || 1);
  const canRetry = Boolean(selected && canRetryUpload(selected.status));
  const canResend = Boolean(selected && canResendNotifications(selected.status));

  async function onDeleteSelected() {
    if (!selectedId) return;
    if (!window.confirm("Möchten Sie den ausgewählten Eintrag löschen?")) return;
    await removeSelected();
  }

  async function onDeleteAll() {
    if (total === 0) return;
    if (!window.confirm("Möchten Sie wirklich die gesamte Historie löschen?")) return;
    await removeAll();
  }

  async function onRetry() {
    if (!selected) return;
    if (!canRetryUpload(selected.status)) {
      window.alert(`Status „${selected.status}“ unterstützt keinen erneuten Upload.`);
      return;
    }
    if (!connected) {
      window.alert("Keine Cloud-Verbindung. Bitte zuerst verbinden.");
      return;
    }
    const lines = [`Upload für „${selected.dir_name}“ erneut starten?`];
    if (selected.error_msg.trim()) {
      lines.push("", `Letzter Fehler: ${selected.error_msg}`);
    }
    if (!window.confirm(lines.join("\n"))) return;
    setActionBusy(true);
    setActionError("");
    setActionMessage("");
    try {
      const message = await retryUpload(selected.id);
      setActionMessage(message);
      await load({ maintainPage: true });
    } catch (err) {
      setActionError(String(err));
    } finally {
      setActionBusy(false);
    }
  }

  async function onOpenResend() {
    if (!selected || !canResendNotifications(selected.status)) {
      window.alert("Nur erfolgreiche Uploads unterstützen einen erneuten Versand.");
      return;
    }
    try {
      setSandboxWarnings(await getSandboxWarnings());
    } catch {
      setSandboxWarnings([]);
    }
    setResendOpen(true);
  }

  async function onResendSend(opts: {
    sendEmail: boolean;
    sendSms: boolean;
    shareLink: string;
  }) {
    if (!selected) return;
    try {
      const delivered = await channelsDelivered(selected.id, opts.sendEmail, opts.sendSms);
      if (delivered.length) {
        const parts: string[] = [];
        if (delivered.includes("email")) {
          parts.push("E-Mail wurde bereits als gesendet markiert");
        }
        if (delivered.includes("sms")) {
          parts.push("SMS wurde bereits als zugestellt markiert");
        }
        if (!window.confirm(`${parts.join(". ")}.\n\nTrotzdem erneut senden?`)) {
          return;
        }
      }
    } catch {
      // continue; backend validates again
    }
    setActionBusy(true);
    setActionError("");
    setActionMessage("");
    try {
      const result = await resendHistoryNotifications(
        selected.id,
        contactEmail,
        contactPhone,
        opts.shareLink,
        opts.sendEmail,
        opts.sendSms,
      );
      setResendOpen(false);
      if (result.had_failures) {
        setActionError(result.message);
      } else {
        setActionMessage(result.message);
      }
      await load({ maintainPage: true });
    } catch (err) {
      setActionError(String(err));
    } finally {
      setActionBusy(false);
    }
  }

  async function onManualStatus(action: string) {
    setStatusMenuOpen(false);
    if (!selected) return;
    try {
      const warnings = await getManualStatusWarnings(selected.id, action);
      if (warnings.length) {
        if (!window.confirm(`${warnings.map((w) => `• ${w}`).join("\n")}\n\nFortfahren?`)) {
          return;
        }
      }
    } catch (err) {
      setActionError(String(err));
      return;
    }
    const reason = window.prompt(
      `Aktion: ${action}\nOptionaler Grund (leer lassen zum Überspringen):`,
      "",
    );
    if (reason === null) return;
    setActionBusy(true);
    setActionError("");
    setActionMessage("");
    try {
      const updated = await setManualStatus(selected.id, action, reason);
      setActionMessage(`Status manuell gesetzt: ${updated.overall_status || action}`);
      await load({ maintainPage: true });
    } catch (err) {
      setActionError(String(err));
    } finally {
      setActionBusy(false);
    }
  }

  async function onSaveContact() {
    if (!selected) return;
    setActionBusy(true);
    setActionError("");
    setActionMessage("");
    try {
      await saveHistoryContact(selected.id, contactEmail, contactPhone);
      setContactDirty(false);
      setActionMessage("Kontaktdaten gespeichert.");
      await load({ maintainPage: true });
    } catch (err) {
      setActionError(String(err));
    } finally {
      setActionBusy(false);
    }
  }

  async function onSyncSms() {
    setActionBusy(true);
    setActionError("");
    setActionMessage("");
    try {
      const n = await syncSmsJournal();
      setActionMessage(
        n ? `SMS-Journal: ${n} Einträge aktualisiert.` : "SMS-Journal: keine Änderungen.",
      );
      await load({ maintainPage: true });
    } catch (err) {
      setActionError(String(err));
    } finally {
      setActionBusy(false);
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border/70 bg-card-elevated/40 px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold tracking-tight text-foreground">
            Upload-Historie
          </h2>
          <p className="text-xs text-muted">
            {total} Einträge{loading ? " · laden…" : ""}
          </p>
        </div>

        <div className="relative min-w-[12rem] flex-1 sm:max-w-xs">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
          <Input
            type="search"
            className="h-8 pl-8 text-xs"
            placeholder="Suchen…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={!canRetry || actionBusy}
          title="Archivierten Auftrag erneut in die Upload-Warteschlange legen"
          onClick={() => void onRetry()}
        >
          <RefreshCw className="h-3.5 w-3.5" />
          Erneut
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={!canResend || actionBusy}
          title="E-Mail/SMS für einen erfolgreichen Upload erneut senden"
          onClick={() => void onOpenResend()}
        >
          Erneut senden…
        </Button>
        <div className="relative">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!selected || actionBusy}
            title="Gesamtstatus manuell setzen"
            onClick={() => setStatusMenuOpen((open) => !open)}
          >
            Status
            <MoreHorizontal className="h-3.5 w-3.5" />
          </Button>
          {statusMenuOpen && selected ? (
            <div className="absolute right-0 z-20 mt-1 min-w-[14rem] overflow-hidden rounded-lg border border-border bg-card py-1 shadow-lg">
              {MANUAL_ACTIONS.map(([action, label]) => (
                <button
                  key={action}
                  type="button"
                  className="block w-full px-3 py-2 text-left text-sm text-foreground hover:bg-primary-soft"
                  onClick={() => void onManualStatus(action)}
                >
                  {label}
                </button>
              ))}
            </div>
          ) : null}
        </div>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={actionBusy}
          title="Offene SMS-Status mit dem Seven.io-Journal abgleichen"
          onClick={() => void onSyncSms()}
        >
          SMS-Journal
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
          disabled={!selectedId}
          onClick={() => void onDeleteSelected()}
          title="Ausgewählten Eintrag löschen"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
          disabled={total === 0}
          onClick={() => void onDeleteAll()}
        >
          Alle löschen
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {error ? <p className="mb-2 text-sm text-destructive">{error}</p> : null}
        {actionError ? (
          <p className="mb-2 whitespace-pre-wrap text-sm text-destructive">
            {actionError}
          </p>
        ) : null}
        {actionMessage ? (
          <p className="mb-2 whitespace-pre-wrap text-sm text-muted">
            {actionMessage}
          </p>
        ) : null}

        <div className="overflow-hidden rounded-xl border border-border ams-surface shadow-sm backdrop-blur-sm">
          <div
            className="grid grid-cols-[9rem_1fr_10rem_1fr] gap-2 px-3 py-2.5 text-xs font-semibold tracking-wide text-muted uppercase"
            style={{ background: "var(--ams-table-head)" }}
          >
            <span>Datum</span>
            <span>Name</span>
            <span>Status</span>
            <span>Fehler</span>
          </div>
          <VirtualList
            items={items}
            rowHeight={44}
            height={Math.min(480, Math.max(200, items.length * 44 + 8))}
            getKey={(item) => item.id}
            className="bg-card/40"
            empty={
              <div className="px-3 py-10 text-center text-sm text-muted">
                Keine Historieneinträge.
              </div>
            }
            renderRow={(item) => {
              const active = item.id === selectedId;
              return (
                <div
                  role="button"
                  tabIndex={0}
                  className={cn(
                    "grid h-full cursor-pointer grid-cols-[9rem_1fr_10rem_1fr] items-center gap-2 px-3 text-sm transition-colors",
                    active
                      ? "bg-[var(--ams-row-active)]"
                      : "hover:bg-[var(--ams-row-hover)]",
                  )}
                  onClick={() => select(item.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      select(item.id);
                    }
                  }}
                >
                  <span className="whitespace-nowrap text-muted">
                    {formatHistoryDate(item.last_updated)}
                  </span>
                  <span className="truncate font-medium text-foreground">
                    {item.display_name || historyDisplayName(item)}
                  </span>
                  <span className="inline-flex items-center gap-2">
                    <StatusDot
                      color={overallStatusColor(item.overall_status)}
                      label={item.overall_status}
                    />
                    <span className="truncate">{item.overall_status}</span>
                  </span>
                  <span
                    className="truncate text-destructive"
                    title={item.combined_error}
                  >
                    {item.combined_error}
                  </span>
                </div>
              );
            }}
          />
        </div>

        <div className="mt-3 flex items-center justify-end gap-2 text-sm text-muted">
          <span className="mr-1 tabular-nums">
            Seite {page + 1} / {pageCount}
          </span>
          <Button
            type="button"
            variant="secondary"
            size="icon"
            className="h-8 w-8"
            disabled={page <= 0}
            onClick={() => setPage(page - 1)}
            aria-label="Vorherige Seite"
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="icon"
            className="h-8 w-8"
            disabled={page >= maxPage || total === 0}
            onClick={() => setPage(page + 1)}
            aria-label="Nächste Seite"
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>

        {selected ? (
          <div className="mt-4 grid gap-2 rounded-xl border border-border bg-card/70 p-4 text-sm shadow-sm backdrop-blur-sm">
            <h3 className="m-0 text-sm font-semibold tracking-tight text-foreground">
              Details
            </h3>
            {DETAIL_ROWS.map(([label, valueOf]) => {
              const value = valueOf(selected);
              const showDot =
                label.toLowerCase().includes("status") &&
                label !== "Manueller Status";
              return (
                <div
                  key={label}
                  className="grid grid-cols-[9.5rem_1fr] gap-2 border-b border-border/40 py-1.5 last:border-b-0"
                >
                  <span className="text-xs font-medium text-muted">{label}</span>
                  <span className="inline-flex items-start gap-2 break-all text-foreground">
                    {showDot ? (
                      <StatusDot
                        className="mt-1"
                        color={overallStatusColor(value)}
                        label={value}
                      />
                    ) : null}
                    {value}
                  </span>
                </div>
              );
            })}

            <div className="mt-2 grid gap-3 border-t border-border pt-3">
              <h4 className="m-0 text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
                Kontakt
              </h4>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="grid gap-1.5">
                  <Label htmlFor="history-contact-email">E-Mail</Label>
                  <Input
                    id="history-contact-email"
                    type="email"
                    value={contactEmail}
                    onChange={(e) => {
                      setContactEmail(e.target.value);
                      setContactDirty(true);
                    }}
                  />
                  <p className="text-xs text-muted">
                    Status: {selected.email_status || "—"}
                  </p>
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="history-contact-phone">Telefon</Label>
                  <Input
                    id="history-contact-phone"
                    type="tel"
                    value={contactPhone}
                    onChange={(e) => {
                      setContactPhone(e.target.value);
                      setContactDirty(true);
                    }}
                  />
                  <p className="text-xs text-muted">
                    Status: {selected.sms_status || "—"}
                  </p>
                </div>
              </div>
              <div>
                <Button
                  type="button"
                  size="sm"
                  disabled={!contactDirty || actionBusy}
                  onClick={() => void onSaveContact()}
                >
                  Kontakt speichern
                </Button>
              </div>
            </div>
          </div>
        ) : null}
      </div>

      {resendOpen && selected ? (
        <ResendNotificationsDialog
          entry={selected}
          email={contactEmail}
          phone={contactPhone}
          shareLink={selected.share_link}
          sandboxWarnings={sandboxWarnings}
          cloudConnected={connected}
          busy={actionBusy}
          onClose={() => {
            if (!actionBusy) setResendOpen(false);
          }}
          onSend={onResendSend}
        />
      ) : null}
    </section>
  );
}
