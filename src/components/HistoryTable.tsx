import { useEffect, useMemo, useRef, useState } from "react";
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
import { HistoryStatusChips, StatusChip } from "./StatusChip";
import { VirtualList } from "./VirtualList";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { UPLOAD_HISTORY_UPDATE } from "@/lib/events";
import { showAppToast } from "@/lib/toast";
import {
  canResendNotifications,
  canRetryUpload,
  cn,
  extraNumber,
  formatHistoryDate,
  formatManualStatusSummary,
  formatResendHistorySummary,
  historyDisplayName,
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
import { useUiStore } from "@/store/uiStore";

const MANUAL_ACTIONS: Array<[string, string]> = [
  ["Komplett", "Als Komplett markieren"],
  ["Versendet", "Als Versendet markieren"],
  ["Problem auflösen", "Problem auflösen"],
];

/** Detail fields — pipeline status lives in HistoryStatusChips only. */
const DETAIL_ROWS: Array<[string, (item: HistoryEntry) => string]> = [
  ["Verzeichnis", (i) => i.dir_name || "—"],
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
  const showError = useUiStore((s) => s.showError);
  const confirm = useUiStore((s) => s.confirm);
  const prompt = useUiStore((s) => s.prompt);

  const [contactEmail, setContactEmail] = useState("");
  const [contactPhone, setContactPhone] = useState("");
  const [contactDirty, setContactDirty] = useState(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [resendOpen, setResendOpen] = useState(false);
  const [sandboxWarnings, setSandboxWarnings] = useState<string[]>([]);
  const listHostRef = useRef<HTMLDivElement>(null);
  const statusMenuRef = useRef<HTMLDivElement>(null);
  const moreMenuRef = useRef<HTMLDivElement>(null);
  const [listHeight, setListHeight] = useState(360);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const el = listHostRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height ?? 0;
      if (h > 0) setListHeight(Math.floor(h));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

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

  useEffect(() => {
    if (!statusMenuOpen && !moreOpen) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        setStatusMenuOpen(false);
        setMoreOpen(false);
      }
    }
    function onPointerDown(e: MouseEvent) {
      const target = e.target as Node;
      if (
        statusMenuOpen &&
        statusMenuRef.current &&
        !statusMenuRef.current.contains(target)
      ) {
        setStatusMenuOpen(false);
      }
      if (moreOpen && moreMenuRef.current && !moreMenuRef.current.contains(target)) {
        setMoreOpen(false);
      }
    }
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [statusMenuOpen, moreOpen]);

  const selected = useMemo(
    () => items.find((item) => item.id === selectedId) ?? null,
    [items, selectedId],
  );

  useEffect(() => {
    setContactEmail(selected?.email ?? "");
    setContactPhone(selected?.phone ?? "");
    setContactDirty(false);
    setStatusMenuOpen(false);
    setMoreOpen(false);
  }, [selected?.id, selected?.email, selected?.phone]);

  const maxPage = Math.max(0, Math.ceil(total / pageSize) - 1 || 0);
  const pageCount = Math.max(1, Math.ceil(total / pageSize) || 1);
  const canRetry = Boolean(selected && canRetryUpload(selected.status));
  const canResend = Boolean(selected && canResendNotifications(selected.status));

  const retryTitle = (() => {
    if (actionBusy) return "Bitte warten…";
    if (!selected) return "Eintrag auswählen, um erneut hochzuladen";
    if (!canRetryUpload(selected.status)) {
      return `Nicht verfügbar bei Status „${selected.status || "—"}“ (nur Fehler/Abgebrochen)`;
    }
    if (!connected) return "Keine Cloud-Verbindung — bitte zuerst verbinden";
    return "Archivierten Auftrag erneut in die Upload-Warteschlange legen";
  })();

  const resendTitle = (() => {
    if (actionBusy) return "Bitte warten…";
    if (!selected) return "Eintrag auswählen, um erneut zu senden";
    if (!canResendNotifications(selected.status)) {
      return `Nicht verfügbar bei Status „${selected.status || "—"}“ (nur erfolgreiche Uploads)`;
    }
    return "E-Mail/SMS für einen erfolgreichen Upload erneut senden";
  })();

  const statusMenuTitle = (() => {
    if (actionBusy) return "Bitte warten…";
    if (!selected) return "Eintrag auswählen, um den Status zu setzen";
    return "Gesamtstatus manuell setzen";
  })();

  const deleteTitle = selectedId
    ? "Ausgewählten Eintrag löschen"
    : "Eintrag auswählen, um zu löschen";

  const emptyMessage = loading
    ? "Historie wird geladen…"
    : search.trim()
      ? "Keine Treffer für die Suche."
      : "Keine Historieneinträge.";

  async function onDeleteSelected() {
    if (!selectedId) return;
    const ok = await confirm("Möchten Sie den ausgewählten Eintrag löschen?", {
      title: "Eintrag löschen",
      primaryLabel: "Löschen",
      destructive: true,
    });
    if (!ok) return;
    await removeSelected();
    showAppToast("Eintrag gelöscht.", { tone: "success" });
  }

  async function onDeleteAll() {
    if (total === 0) return;
    setMoreOpen(false);
    const ok = await confirm(
      "Möchten Sie wirklich die gesamte Historie löschen?",
      {
        title: "Historie leeren",
        primaryLabel: "Alles löschen",
        destructive: true,
      },
    );
    if (!ok) return;
    await removeAll();
    showAppToast("Historie geleert.", { tone: "success" });
  }

  async function onRetry() {
    if (!selected) return;
    if (!canRetryUpload(selected.status)) {
      showError(
        `Status „${selected.status}“ unterstützt keinen erneuten Upload.`,
        "Erneut hochladen",
      );
      return;
    }
    if (!connected) {
      showError("Keine Cloud-Verbindung. Bitte zuerst verbinden.", "Erneut hochladen");
      return;
    }
    const lines = [`Upload für „${selected.dir_name}“ erneut starten?`];
    if (selected.error_msg.trim()) {
      lines.push("", `Letzter Fehler: ${selected.error_msg}`);
    }
    const ok = await confirm(lines.join("\n"), {
      title: "Upload erneut",
      primaryLabel: "Erneut starten",
    });
    if (!ok) return;
    setActionBusy(true);
    try {
      const message = await retryUpload(selected.id);
      showAppToast(message, { tone: "success", title: "Upload" });
      await load({ maintainPage: true });
    } catch (err) {
      showError(String(err), "Upload erneut");
    } finally {
      setActionBusy(false);
    }
  }

  async function onOpenResend() {
    if (!selected || !canResendNotifications(selected.status)) {
      showError(
        "Nur erfolgreiche Uploads unterstützen einen erneuten Versand.",
        "Erneut senden",
      );
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
      const delivered = await channelsDelivered(
        selected.id,
        opts.sendEmail,
        opts.sendSms,
      );
      if (delivered.length) {
        const parts: string[] = [];
        if (delivered.includes("email")) {
          parts.push("E-Mail wurde bereits als gesendet markiert");
        }
        if (delivered.includes("sms")) {
          parts.push("SMS wurde bereits als zugestellt markiert");
        }
        const ok = await confirm(
          `${parts.join(". ")}.\n\nTrotzdem erneut senden?`,
          { title: "Erneut senden", primaryLabel: "Erneut senden" },
        );
        if (!ok) return;
      }
    } catch {
      // continue; backend validates again
    }
    setActionBusy(true);
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
        showError(result.message, "Erneut senden");
      } else {
        showAppToast(result.message, { tone: "success", title: "Benachrichtigung" });
      }
      await load({ maintainPage: true });
    } catch (err) {
      showError(String(err), "Erneut senden");
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
        const ok = await confirm(
          `${warnings.map((w) => `• ${w}`).join("\n")}\n\nFortfahren?`,
          { title: "Status setzen", primaryLabel: "Fortfahren" },
        );
        if (!ok) return;
      }
    } catch (err) {
      showError(String(err), "Status setzen");
      return;
    }
    const reason = await prompt(`Aktion: ${action}`, {
      title: "Status setzen",
      hint: "Optionaler Grund (leer lassen zum Überspringen)",
      placeholder: "Grund…",
      primaryLabel: "Status setzen",
    });
    if (reason === null) return;
    setActionBusy(true);
    try {
      const updated = await setManualStatus(selected.id, action, reason);
      showAppToast(`Status manuell gesetzt: ${updated.overall_status || action}`, {
        tone: "success",
      });
      await load({ maintainPage: true });
    } catch (err) {
      showError(String(err), "Status setzen");
    } finally {
      setActionBusy(false);
    }
  }

  async function onSaveContact() {
    if (!selected) return;
    setActionBusy(true);
    try {
      await saveHistoryContact(selected.id, contactEmail, contactPhone);
      setContactDirty(false);
      showAppToast("Kontaktdaten gespeichert.", { tone: "success" });
      await load({ maintainPage: true });
    } catch (err) {
      showError(String(err), "Kontakt");
    } finally {
      setActionBusy(false);
    }
  }

  async function onSyncSms() {
    setMoreOpen(false);
    setActionBusy(true);
    try {
      const n = await syncSmsJournal();
      showAppToast(
        n
          ? `SMS-Journal: ${n} Einträge aktualisiert.`
          : "SMS-Journal: keine Änderungen.",
        { tone: "success", title: "SMS-Journal" },
      );
      await load({ maintainPage: true });
    } catch (err) {
      showError(String(err), "SMS-Journal");
    } finally {
      setActionBusy(false);
    }
  }

  function renderDetailRows(rows: typeof DETAIL_ROWS) {
    return rows.map(([label, valueOf]) => {
      const value = valueOf(selected!);
      return (
        <div
          key={label}
          className="grid grid-cols-[7.5rem_1fr] gap-2 border-b border-border/40 py-1.5 last:border-b-0 lg:grid-cols-[9.5rem_1fr]"
        >
          <span className="text-xs font-medium text-muted">{label}</span>
          <span className="break-all text-foreground">{value}</span>
        </div>
      );
    });
  }

  return (
    <section className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border/70 bg-card-elevated/40 px-4 py-3">
        <div className="min-w-0 flex-1 basis-full sm:basis-auto">
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
            aria-label="Historie durchsuchen"
          />
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!canRetry || actionBusy}
            title={retryTitle}
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
            title={resendTitle}
            onClick={() => void onOpenResend()}
          >
            Erneut senden…
          </Button>
          <div className="relative" ref={statusMenuRef}>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={!selected || actionBusy}
              title={statusMenuTitle}
              aria-expanded={statusMenuOpen}
              aria-haspopup="menu"
              onClick={() => {
                setMoreOpen(false);
                setStatusMenuOpen((open) => !open);
              }}
            >
              Status
              <MoreHorizontal className="h-3.5 w-3.5" />
            </Button>
            {statusMenuOpen && selected ? (
              <div
                role="menu"
                className="absolute right-0 z-20 mt-1 min-w-[14rem] overflow-hidden rounded-lg border border-border bg-card py-1 shadow-lg"
              >
                {MANUAL_ACTIONS.map(([action, label]) => (
                  <button
                    key={action}
                    type="button"
                    role="menuitem"
                    className="block w-full px-3 py-2 text-left text-sm text-foreground hover:bg-primary-soft"
                    onClick={() => void onManualStatus(action)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
          <div className="relative" ref={moreMenuRef}>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={actionBusy}
              title="Weitere Aktionen"
              aria-expanded={moreOpen}
              aria-haspopup="menu"
              onClick={() => {
                setStatusMenuOpen(false);
                setMoreOpen((open) => !open);
              }}
            >
              <MoreHorizontal className="h-3.5 w-3.5" />
            </Button>
            {moreOpen ? (
              <div
                role="menu"
                className="absolute right-0 z-20 mt-1 min-w-[12rem] overflow-hidden rounded-lg border border-border bg-card py-1 shadow-lg"
              >
                <button
                  type="button"
                  role="menuitem"
                  className="block w-full px-3 py-2 text-left text-sm text-foreground hover:bg-primary-soft"
                  onClick={() => void onSyncSms()}
                >
                  SMS-Journal abgleichen
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="block w-full px-3 py-2 text-left text-sm text-destructive hover:bg-destructive/10 disabled:opacity-50"
                  disabled={total === 0}
                  onClick={() => void onDeleteAll()}
                >
                  Alle löschen
                </button>
              </div>
            ) : null}
          </div>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
            disabled={!selectedId}
            onClick={() => void onDeleteSelected()}
            title={deleteTitle}
            aria-label={deleteTitle}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {error ? (
        <p className="shrink-0 border-b border-border/60 px-4 py-2 text-sm text-destructive">
          {error}
        </p>
      ) : null}

      <div className="flex min-h-0 flex-1">
        <div className="flex min-h-0 w-full min-w-0 flex-col lg:w-[min(52%,28rem)] lg:border-r lg:border-border">
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            <div
              className="shrink-0 grid grid-cols-[7.5rem_1fr_auto] gap-2 px-3 py-2.5 text-xs font-semibold tracking-wide text-muted uppercase"
              style={{ background: "var(--ams-table-head)" }}
            >
              <span>Datum</span>
              <span>Name</span>
              <span>Status</span>
            </div>
            <div ref={listHostRef} className="min-h-0 flex-1">
              <VirtualList
                items={items}
                rowHeight={48}
                height={listHeight}
                getKey={(item) => item.id}
                className="bg-card/40"
                empty={
                  <div className="px-3 py-10 text-center text-sm text-muted">
                    {emptyMessage}
                  </div>
                }
                renderRow={(item) => {
                  const active = item.id === selectedId;
                  return (
                    <div
                      role="button"
                      tabIndex={0}
                      aria-selected={active}
                      aria-label={`${item.display_name || historyDisplayName(item)}, Status ${item.overall_status || "—"}`}
                      className={cn(
                        "grid h-full cursor-pointer grid-cols-[7.5rem_1fr_auto] items-center gap-2 px-3 text-sm transition-colors",
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
                      <span className="whitespace-nowrap text-xs text-muted">
                        {formatHistoryDate(item.last_updated)}
                      </span>
                      <div className="min-w-0">
                        <p className="truncate font-medium text-foreground">
                          {item.display_name || historyDisplayName(item)}
                        </p>
                        {item.combined_error ? (
                          <p
                            className="truncate text-[11px] text-destructive"
                            title={item.combined_error}
                          >
                            {item.combined_error}
                          </p>
                        ) : null}
                      </div>
                      <StatusChip
                        status={item.overall_status}
                        channel="overall"
                        compact
                      />
                    </div>
                  );
                }}
              />
            </div>
          </div>

          <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border/70 px-3 py-2 text-sm text-muted">
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
        </div>

        <aside className="hidden min-h-0 min-w-0 flex-1 overflow-y-auto p-4 lg:block">
          {selected ? (
            <div className="grid gap-2 rounded-xl border border-border bg-card/70 p-4 text-sm shadow-sm backdrop-blur-sm">
              <div className="mb-1 flex flex-wrap items-start justify-between gap-2">
                <h3 className="m-0 text-sm font-semibold tracking-tight text-foreground">
                  Details
                </h3>
                <HistoryStatusChips entry={selected} />
              </div>
              {renderDetailRows(DETAIL_ROWS)}

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
          ) : (
            <div className="flex h-full min-h-[12rem] items-center justify-center rounded-xl border border-dashed border-border/80 bg-card/40 px-6 text-center text-sm text-muted">
              Eintrag auswählen, um Details zu sehen.
            </div>
          )}
        </aside>
      </div>

      {selected ? (
        <div className="border-t border-border p-4 lg:hidden">
          <h3 className="mb-2 text-sm font-semibold tracking-tight text-foreground">
            Details
          </h3>
          <HistoryStatusChips entry={selected} className="mb-3" />
          <div className="grid gap-2 text-sm">
            {renderDetailRows(DETAIL_ROWS)}
          </div>
        </div>
      ) : null}

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
