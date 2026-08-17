import { useCallback, useEffect, useRef, useState, type ClipboardEvent, type FormEvent, type Ref } from "react";
import {
  Check,
  ClipboardPaste,
  ListChecks,
  Pencil,
  RotateCcw,
  Search,
  Trash2,
  UserPlus,
} from "lucide-react";
import { FolderSelectionModal } from "./FolderSelectionModal";
import { BatchAssignDialog } from "./BatchAssignDialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn, formatHistoryDate } from "@/lib/utils";
import type { Customer } from "@/lib/tauri";
import { useCustomerStore, type CustomerFilter } from "@/store/customerStore";
import { useUiStore } from "@/store/uiStore";

type FormState = {
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
};

const EMPTY_FORM: FormState = {
  vorname: "",
  nachname: "",
  email: "",
  telefon: "",
};

type ClipboardCustomer = {
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
};

function parseCustomerJsonPaste(text: string): ClipboardCustomer | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const data = JSON.parse(trimmed) as Record<string, unknown>;
    if (typeof data !== "object" || data === null || Array.isArray(data)) {
      return null;
    }
    const vorname = data.vorname;
    const nachname = data.name ?? data.nachname;
    const email = data.email;
    const telefon = data.telefon;
    if (
      typeof vorname !== "string" ||
      typeof nachname !== "string" ||
      typeof email !== "string"
    ) {
      return null;
    }
    return {
      vorname: vorname.trim(),
      nachname: nachname.trim(),
      email: email.trim(),
      telefon: typeof telefon === "string" ? telefon.trim() : "",
    };
  } catch {
    return null;
  }
}

async function readClipboardText(): Promise<string> {
  try {
    if (navigator.clipboard?.readText && document.hasFocus()) {
      return await navigator.clipboard.readText();
    }
  } catch {
    /* clipboard denied */
  }
  return "";
}

function validateForm(form: FormState): string | null {
  if (form.vorname.trim().length < 2) return "Vorname ist erforderlich (min. 2 Zeichen).";
  if (form.nachname.trim().length < 2) return "Nachname ist erforderlich (min. 2 Zeichen).";
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(form.email.trim())) {
    return "Bitte eine gültige E-Mail-Adresse eingeben.";
  }
  return null;
}

function customerLabel(customer: Pick<Customer, "vorname" | "nachname">): string {
  return `${customer.vorname} ${customer.nachname}`.trim();
}

/** Folder name from a stored marker path (`…/Job-1/_fertig.txt` → `Job-1`). */
function assignedDirName(assignedPath: string): string {
  const parts = assignedPath
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter((part) => part.length > 0);
  if (parts.length === 0) return "";
  const last = parts[parts.length - 1] ?? "";
  if (last === "_fertig.txt" || last === "_in_verarbeitung.txt") {
    return parts[parts.length - 2] ?? last;
  }
  return last;
}

export function CustomersPanel() {
  const items = useCustomerStore((s) => s.items);
  const history = useCustomerStore((s) => s.history);
  const search = useCustomerStore((s) => s.search);
  const filter = useCustomerStore((s) => s.filter);
  const view = useCustomerStore((s) => s.view);
  const openCount = useCustomerStore((s) => s.openCount);
  const highlightId = useCustomerStore((s) => s.highlightId);
  const loading = useCustomerStore((s) => s.loading);
  const setSearch = useCustomerStore((s) => s.setSearch);
  const setFilter = useCustomerStore((s) => s.setFilter);
  const setView = useCustomerStore((s) => s.setView);
  const load = useCustomerStore((s) => s.load);
  const loadHistory = useCustomerStore((s) => s.loadHistory);
  const add = useCustomerStore((s) => s.add);
  const update = useCustomerStore((s) => s.update);
  const remove = useCustomerStore((s) => s.remove);
  const setProcessed = useCustomerStore((s) => s.setProcessed);
  const assign = useCustomerStore((s) => s.assign);
  const confirm = useUiStore((s) => s.confirm);

  const [intakeOpen, setIntakeOpen] = useState(false);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [formError, setFormError] = useState("");
  const [formBusy, setFormBusy] = useState(false);
  const [assignAfterSave, setAssignAfterSave] = useState(false);
  const [intakeSavedCount, setIntakeSavedCount] = useState(0);
  const [intakeFocusKey, setIntakeFocusKey] = useState(0);
  const [clipboardCustomer, setClipboardCustomer] = useState<ClipboardCustomer | null>(
    null,
  );
  const lastAppliedClipboardRef = useRef("");
  const vornameRef = useRef<HTMLInputElement>(null);

  const [exportingId, setExportingId] = useState<string | null>(null);
  const [exportingLabel, setExportingLabel] = useState("");
  const [exportingVorname, setExportingVorname] = useState("");
  const [exportingNachname, setExportingNachname] = useState("");
  const [exportingEmail, setExportingEmail] = useState("");
  const [assignBusy, setAssignBusy] = useState(false);
  const [editing, setEditing] = useState<Customer | null>(null);
  const [editForm, setEditForm] = useState<FormState>(EMPTY_FORM);
  const [editError, setEditError] = useState("");
  const [editBusy, setEditBusy] = useState(false);
  const [batchOpen, setBatchOpen] = useState(false);

  function startAssign(customer: Pick<Customer, "id" | "vorname" | "nachname" | "email">) {
    setExportingId(customer.id);
    setExportingLabel(customerLabel(customer));
    setExportingVorname(customer.vorname);
    setExportingNachname(customer.nachname);
    setExportingEmail(customer.email);
  }

  const checkClipboard = useCallback(async () => {
    try {
      const text = await readClipboardText();
      const trimmed = text.trim();
      const parsed = parseCustomerJsonPaste(trimmed);
      if (parsed && trimmed !== lastAppliedClipboardRef.current) {
        setClipboardCustomer(parsed);
      } else {
        setClipboardCustomer(null);
      }
    } catch {
      setClipboardCustomer(null);
    }
  }, []);

  useEffect(() => {
    void load();
    void loadHistory();
  }, [load, loadHistory]);

  useEffect(() => {
    void checkClipboard();
    const onFocus = () => void checkClipboard();
    const onVisibility = () => {
      if (document.visibilityState === "visible") void checkClipboard();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    const interval = window.setInterval(() => void checkClipboard(), 1500);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
      window.clearInterval(interval);
    };
  }, [checkClipboard]);

  useEffect(() => {
    if (!intakeOpen) return;
    const id = window.setTimeout(() => vornameRef.current?.focus(), 40);
    return () => window.clearTimeout(id);
  }, [intakeOpen, intakeFocusKey]);

  function applyClipboard(parsed: ClipboardCustomer) {
    setForm({
      vorname: parsed.vorname,
      nachname: parsed.nachname,
      email: parsed.email,
      telefon: parsed.telefon,
    });
    setFormError("");
  }

  function resetIntakeDialog() {
    setIntakeOpen(false);
    setForm(EMPTY_FORM);
    setFormError("");
    setAssignAfterSave(false);
    setIntakeSavedCount(0);
    lastAppliedClipboardRef.current = "";
    void checkClipboard();
  }

  function openIntake(prefill?: ClipboardCustomer) {
    setFormError("");
    setAssignAfterSave(false);
    setIntakeSavedCount(0);
    if (prefill) {
      applyClipboard(prefill);
      lastAppliedClipboardRef.current = JSON.stringify(prefill);
      setClipboardCustomer(null);
    } else {
      setForm(EMPTY_FORM);
    }
    setIntakeOpen(true);
  }

  function closeIntake() {
    if (formBusy) return;
    resetIntakeDialog();
  }

  async function submitIntake(parsed?: ClipboardCustomer) {
    const payload = parsed ?? form;
    const err = validateForm(payload);
    if (err) {
      setFormError(err);
      if (parsed) {
        applyClipboard(parsed);
        setIntakeOpen(true);
      }
      return null;
    }
    setFormBusy(true);
    setFormError("");
    try {
      return await add(
        payload.vorname,
        payload.nachname,
        payload.email,
        payload.telefon,
      );
    } catch {
      return null;
    } finally {
      setFormBusy(false);
    }
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const shouldAssign = assignAfterSave;
    const customer = await submitIntake();
    if (!customer) return;

    setForm(EMPTY_FORM);
    if (shouldAssign) {
      resetIntakeDialog();
      startAssign(customer);
      return;
    }

    setIntakeSavedCount((n) => n + 1);
    setIntakeFocusKey((n) => n + 1);
    void checkClipboard();
  }

  function onFormPaste(e: ClipboardEvent) {
    const text = e.clipboardData.getData("text");
    const parsed = parseCustomerJsonPaste(text);
    if (!parsed) return;
    e.preventDefault();
    applyClipboard(parsed);
    lastAppliedClipboardRef.current = text.trim();
    setClipboardCustomer(null);
  }

  async function onQuickAddFromClipboard() {
    if (!clipboardCustomer) return;
    const snapshot = clipboardCustomer;
    lastAppliedClipboardRef.current = JSON.stringify(snapshot);
    const customer = await submitIntake(snapshot);
    if (customer) setClipboardCustomer(null);
  }

  async function onAssign(folderPath: string) {
    if (!exportingId) return;
    setAssignBusy(true);
    try {
      await assign(exportingId, folderPath);
      setExportingId(null);
    } catch {
      /* store toasts */
    } finally {
      setAssignBusy(false);
    }
  }

  function openEdit(customer: Customer) {
    setEditing(customer);
    setEditForm({
      vorname: customer.vorname,
      nachname: customer.nachname,
      email: customer.email,
      telefon: customer.telefon,
    });
    setEditError("");
  }

  async function saveEdit() {
    if (!editing) return;
    const err = validateForm(editForm);
    if (err) {
      setEditError(err);
      return;
    }
    setEditBusy(true);
    try {
      await update({
        ...editing,
        vorname: editForm.vorname.trim(),
        nachname: editForm.nachname.trim(),
        email: editForm.email.trim(),
        telefon: editForm.telefon.trim(),
      });
      setEditing(null);
    } catch {
      /* store */
    } finally {
      setEditBusy(false);
    }
  }

  async function deleteEdit() {
    if (!editing) return;
    const ok = await confirm("Diesen Kunden wirklich löschen?", {
      title: "Kunde löschen",
      primaryLabel: "Löschen",
      destructive: true,
    });
    if (!ok) return;
    setEditBusy(true);
    try {
      await remove(editing.id);
      setEditing(null);
    } catch {
      /* store */
    } finally {
      setEditBusy(false);
    }
  }

  async function toggleProcessed(customer: Customer) {
    const next = !customer.processed;
    const ok = await confirm(
      next
        ? `${customerLabel(customer)} als erledigt markieren, ohne Ordner zuzuweisen?`
        : `${customerLabel(customer)} wieder in die offene Warteschlange legen?`,
      {
        title: next ? "Als erledigt markieren" : "Wieder öffnen",
        primaryLabel: next ? "Als erledigt" : "Wieder öffnen",
      },
    );
    if (!ok) return;
    await setProcessed(customer.id, next);
  }

  const filters: Array<[CustomerFilter, string]> = [
    ["all", "Alle"],
    ["unprocessed", "Offen"],
    ["processed", "Erledigt"],
  ];

  const emptyQueueText = loading
    ? "Laden…"
    : search.trim()
      ? "Keine Treffer für die Suche."
      : filter === "processed"
        ? "Noch keine erledigten Kunden."
        : filter === "all"
          ? "Noch keine Kunden."
          : "Keine offenen Kunden.";

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-x-3 gap-y-2 border-b border-border/70 bg-card-elevated/40 px-4 py-3">
        <Tabs
          value={view}
          onValueChange={(value) => {
            if (value === "queue" || value === "history") setView(value);
          }}
        >
          <TabsList className="h-8" aria-label="Kundenansicht">
            <TabsTrigger value="queue" className="h-6 gap-1.5 px-2.5 text-xs">
              Warteschlange
              {openCount > 0 ? (
                <span className="rounded-full bg-primary-soft px-1.5 text-[10px] font-semibold leading-4 text-primary">
                  {openCount}
                </span>
              ) : null}
            </TabsTrigger>
            <TabsTrigger value="history" className="h-6 gap-1.5 px-2.5 text-xs">
              Zuweisungen
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={openCount === 0}
            onClick={() => setBatchOpen(true)}
            title="Offene Kunden passenden Ordnern zuordnen"
          >
            <ListChecks className="h-3.5 w-3.5" />
            Passende zuweisen…
          </Button>
          <Button type="button" size="sm" onClick={() => openIntake()}>
            <UserPlus className="h-3.5 w-3.5" />
            Aufnehmen…
          </Button>
        </div>
      </div>

      {view === "queue" && clipboardCustomer ? (
        <div className="shrink-0 border-b border-sky-500/25 bg-sky-500/10 px-4 py-2.5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="text-sm text-foreground">
              Zwischenablage:{" "}
              <span className="font-medium">
                {clipboardCustomer.vorname} {clipboardCustomer.nachname}
              </span>
              <span className="ml-1.5 text-xs text-muted">{clipboardCustomer.email}</span>
            </p>
            <div className="flex gap-1.5">
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => openIntake(clipboardCustomer)}
              >
                <ClipboardPaste className="h-3.5 w-3.5" />
                Übernehmen…
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                disabled={formBusy}
                onClick={() => void onQuickAddFromClipboard()}
              >
                Direkt anlegen
              </Button>
            </div>
          </div>
        </div>
      ) : null}

      {view === "queue" ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-4 py-3">
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <div className="relative min-w-[12rem] flex-1">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <Input
                className="h-8 pl-8 text-xs"
                placeholder="Suchen…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                aria-label="Kunden durchsuchen"
              />
            </div>
            <div
              className="flex items-center gap-0.5 rounded-lg border border-border bg-card-elevated p-0.5"
              role="group"
              aria-label="Status filtern"
            >
              {filters.map(([key, label]) => {
                const selected = filter === key;
                return (
                  <button
                    key={key}
                    type="button"
                    aria-pressed={selected}
                    className={cn(
                      "inline-flex h-7 items-center gap-1 rounded-md px-2.5 text-xs font-medium transition-colors",
                      "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      selected
                        ? "bg-card text-foreground shadow-sm ring-1 ring-border"
                        : "text-muted hover:text-foreground",
                    )}
                    onClick={() => setFilter(key)}
                  >
                    {label}
                    {key === "unprocessed" && openCount > 0 ? (
                      <span
                        className={cn(
                          "rounded-full px-1.5 text-[10px] font-semibold leading-4",
                          selected
                            ? "bg-primary-soft text-primary"
                            : "bg-background/80 text-muted",
                        )}
                      >
                        {openCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border">
            {items.length === 0 ? (
              <div className="flex h-full min-h-[12rem] flex-col items-center justify-center gap-3 p-6 text-center">
                <p className="text-sm text-muted">{emptyQueueText}</p>
                {!loading && !search.trim() && filter !== "processed" ? (
                  <Button type="button" onClick={() => openIntake()}>
                    <UserPlus className="h-3.5 w-3.5" />
                    Aufnehmen…
                  </Button>
                ) : null}
              </div>
            ) : (
              <ul className="divide-y divide-border">
                {items.map((customer) => {
                  const highlighted = customer.id === highlightId;
                  return (
                    <li
                      key={customer.id}
                      className={cn(
                        "flex flex-wrap items-center gap-2 px-3 py-2.5 transition-colors sm:flex-nowrap",
                        highlighted
                          ? "bg-[var(--ams-row-active)]"
                          : "hover:bg-[var(--ams-row-hover)]",
                      )}
                    >
                      <div className="min-w-0 flex-1 basis-[11rem]">
                        <p className="truncate text-sm font-medium text-foreground">
                          {customerLabel(customer)}
                          {customer.processed ? (
                            <span className="ml-2 inline-flex items-center rounded-full border border-success/30 bg-success/10 px-1.5 py-px text-[10px] font-medium tracking-wide text-success uppercase">
                              erledigt
                            </span>
                          ) : (
                            <span className="ml-2 inline-flex items-center rounded-full border border-warning/35 bg-warning/10 px-1.5 py-px text-[10px] font-medium tracking-wide text-warning uppercase">
                              offen
                            </span>
                          )}
                        </p>
                        <p className="truncate text-xs text-muted">
                          {customer.email}
                          {customer.telefon ? ` · ${customer.telefon}` : ""}
                        </p>
                      </div>
                      {customer.assigned_path.trim() ? (
                        <div className="min-w-0 flex-1 basis-[12rem]">
                          <p className="truncate text-xs text-muted">
                            {formatHistoryDate(
                              history.find((entry) => entry.customer_id === customer.id)
                                ?.created_at || customer.updated_at,
                            )}
                          </p>
                          <p
                            className="truncate font-mono text-xs text-foreground"
                            title={customer.assigned_path}
                          >
                            {assignedDirName(customer.assigned_path)}
                          </p>
                        </div>
                      ) : null}
                      <div className="flex shrink-0 flex-wrap items-center gap-1">
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          className={
                            customer.processed ? undefined : "border-2 border-primary/50"
                          }
                          onClick={() => startAssign(customer)}
                          title={
                            customer.processed
                              ? "Erneut einem Ordner zuweisen"
                              : "Ordner wählen"
                          }
                        >
                          {customer.processed ? "Erneut zuweisen…" : "Ordner zuweisen…"}
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => openEdit(customer)}
                          title="Bearbeiten"
                          aria-label="Bearbeiten"
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => void toggleProcessed(customer)}
                          title={
                            customer.processed
                              ? "Als offen markieren"
                              : "Als erledigt markieren"
                          }
                          aria-label={
                            customer.processed
                              ? "Als offen markieren"
                              : "Als erledigt markieren"
                          }
                        >
                          {customer.processed ? (
                            <RotateCcw className="h-3.5 w-3.5" />
                          ) : (
                            <Check className="h-3.5 w-3.5" />
                          )}
                        </Button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {history.length === 0 ? (
            <div className="flex min-h-[12rem] flex-col items-center justify-center gap-3 text-center">
              <p className="text-sm text-muted">Noch keine Zuweisungen.</p>
              <Button type="button" variant="secondary" onClick={() => setView("queue")}>
                Zur Warteschlange
              </Button>
            </div>
          ) : (
            <div className="overflow-x-auto rounded-lg border border-border">
              <table className="w-full min-w-[32rem] text-left text-sm">
                <thead className="border-b border-border bg-card-elevated text-xs text-muted">
                  <tr>
                    <th className="px-3 py-2 font-medium">Zeit</th>
                    <th className="px-3 py-2 font-medium">Kunde</th>
                    <th className="px-3 py-2 font-medium">E-Mail</th>
                    <th className="px-3 py-2 font-medium">Pfad</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {history.map((entry) => (
                    <tr key={entry.id} className="hover:bg-[var(--ams-row-hover)]">
                      <td className="px-3 py-2 whitespace-nowrap text-xs text-muted">
                        {formatHistoryDate(entry.created_at)}
                      </td>
                      <td className="px-3 py-2">
                        {entry.vorname} {entry.nachname}
                      </td>
                      <td className="px-3 py-2 text-muted">{entry.email}</td>
                      <td
                        className="max-w-[16rem] truncate px-3 py-2 font-mono text-xs text-muted"
                        title={entry.file_path}
                      >
                        {entry.file_path}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      <Dialog
        open={intakeOpen}
        onOpenChange={(open) => {
          if (!open) closeIntake();
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Kunde aufnehmen</DialogTitle>
            <DialogDescription>
              Formular bleibt offen für den nächsten Kunden.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void onSubmit(e)} onPaste={onFormPaste} className="space-y-3">
            {clipboardCustomer ? (
              <div className="flex items-center justify-between gap-3 rounded-lg border border-sky-500/30 bg-sky-500/10 px-3 py-2.5">
                <p className="text-sm text-foreground">
                  Zwischenablage:{" "}
                  <span className="font-medium">
                    {clipboardCustomer.vorname} {clipboardCustomer.nachname}
                  </span>
                </p>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    applyClipboard(clipboardCustomer);
                    lastAppliedClipboardRef.current = JSON.stringify(clipboardCustomer);
                    setClipboardCustomer(null);
                  }}
                >
                  <ClipboardPaste className="h-3.5 w-3.5" />
                  Einfügen
                </Button>
              </div>
            ) : null}

            <Field
              inputRef={vornameRef}
              label="Vorname *"
              value={form.vorname}
              onChange={(v) => setForm((f) => ({ ...f, vorname: v }))}
            />
            <Field
              label="Nachname *"
              value={form.nachname}
              onChange={(v) => setForm((f) => ({ ...f, nachname: v }))}
            />
            <Field
              label="E-Mail *"
              type="email"
              value={form.email}
              onChange={(v) => setForm((f) => ({ ...f, email: v }))}
            />
            <Field
              label="Telefon (optional)"
              type="tel"
              value={form.telefon}
              onChange={(v) => setForm((f) => ({ ...f, telefon: v }))}
            />

            <label className="flex items-start gap-2 pt-1 text-sm text-foreground">
              <Checkbox
                checked={assignAfterSave}
                onCheckedChange={(v) => setAssignAfterSave(v === true)}
                className="mt-0.5"
              />
              <span>Nach dem Speichern direkt einem Ordner zuweisen</span>
            </label>

            {formError ? <p className="text-xs text-destructive">{formError}</p> : null}

            <DialogFooter className="gap-2 pt-1">
              <Button type="button" variant="secondary" disabled={formBusy} onClick={closeIntake}>
                {intakeSavedCount > 0 ? "Fertig" : "Abbrechen"}
              </Button>
              <Button type="submit" disabled={formBusy}>
                <UserPlus className="h-3.5 w-3.5" />
                {formBusy ? "Speichern…" : "In Warteschlange"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <FolderSelectionModal
        open={Boolean(exportingId)}
        busy={assignBusy}
        customerLabel={exportingLabel || undefined}
        vorname={exportingVorname}
        nachname={exportingNachname}
        email={exportingEmail}
        onClose={() => {
          if (!assignBusy) {
            setExportingId(null);
            setExportingLabel("");
            setExportingVorname("");
            setExportingNachname("");
            setExportingEmail("");
          }
        }}
        onSelect={(path) => void onAssign(path)}
      />

      <BatchAssignDialog open={batchOpen} onClose={() => setBatchOpen(false)} />

      <Dialog
        open={Boolean(editing)}
        onOpenChange={(v) => {
          if (!v && !editBusy) setEditing(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Kunde bearbeiten</DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <Field
              label="Vorname *"
              value={editForm.vorname}
              onChange={(v) => setEditForm((f) => ({ ...f, vorname: v }))}
            />
            <Field
              label="Nachname *"
              value={editForm.nachname}
              onChange={(v) => setEditForm((f) => ({ ...f, nachname: v }))}
            />
            <Field
              label="E-Mail *"
              type="email"
              value={editForm.email}
              onChange={(v) => setEditForm((f) => ({ ...f, email: v }))}
            />
            <Field
              label="Telefon"
              type="tel"
              value={editForm.telefon}
              onChange={(v) => setEditForm((f) => ({ ...f, telefon: v }))}
            />
            {editError ? <p className="text-xs text-destructive">{editError}</p> : null}
          </div>
          <DialogFooter className="gap-2 sm:justify-between">
            <Button
              type="button"
              variant="destructive"
              disabled={editBusy}
              onClick={() => void deleteEdit()}
            >
              <Trash2 className="h-3.5 w-3.5" />
              Löschen
            </Button>
            <div className="flex gap-2">
              <Button
                type="button"
                variant="secondary"
                disabled={editBusy}
                onClick={() => setEditing(null)}
              >
                Abbrechen
              </Button>
              <Button type="button" disabled={editBusy} onClick={() => void saveEdit()}>
                Speichern
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  type = "text",
  inputRef,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  inputRef?: Ref<HTMLInputElement>;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <Input
        ref={inputRef}
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
      />
    </div>
  );
}
