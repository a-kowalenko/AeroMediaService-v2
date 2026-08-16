import { useCallback, useEffect, useRef, useState } from "react";
import {
  Check,
  ClipboardPaste,
  Pencil,
  Search,
  Trash2,
  UserPlus,
} from "lucide-react";
import { FolderSelectionModal } from "./FolderSelectionModal";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn, formatHistoryDate } from "@/lib/utils";
import type { Customer } from "@/lib/tauri";
import { useCustomerStore, type CustomerFilter } from "@/store/customerStore";
import { useUiStore } from "@/store/uiStore";
import { showAppToast } from "@/lib/toast";

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

export function CustomersPanel() {
  const items = useCustomerStore((s) => s.items);
  const history = useCustomerStore((s) => s.history);
  const search = useCustomerStore((s) => s.search);
  const filter = useCustomerStore((s) => s.filter);
  const loading = useCustomerStore((s) => s.loading);
  const error = useCustomerStore((s) => s.error);
  const message = useCustomerStore((s) => s.message);
  const setSearch = useCustomerStore((s) => s.setSearch);
  const setFilter = useCustomerStore((s) => s.setFilter);
  const load = useCustomerStore((s) => s.load);
  const loadHistory = useCustomerStore((s) => s.loadHistory);
  const add = useCustomerStore((s) => s.add);
  const update = useCustomerStore((s) => s.update);
  const remove = useCustomerStore((s) => s.remove);
  const setProcessed = useCustomerStore((s) => s.setProcessed);
  const assign = useCustomerStore((s) => s.assign);
  const confirm = useUiStore((s) => s.confirm);

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [formError, setFormError] = useState("");
  const [formBusy, setFormBusy] = useState(false);
  const [clipboardCustomer, setClipboardCustomer] = useState<ClipboardCustomer | null>(
    null,
  );
  const lastAppliedClipboardRef = useRef("");

  const [exportingId, setExportingId] = useState<string | null>(null);
  const [assignBusy, setAssignBusy] = useState(false);
  const [editing, setEditing] = useState<Customer | null>(null);
  const [editForm, setEditForm] = useState<FormState>(EMPTY_FORM);
  const [editError, setEditError] = useState("");
  const [editBusy, setEditBusy] = useState(false);

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

  function applyClipboard(parsed: ClipboardCustomer) {
    setForm({
      vorname: parsed.vorname,
      nachname: parsed.nachname,
      email: parsed.email,
      telefon: parsed.telefon,
    });
    setFormError("");
  }

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    const err = validateForm(form);
    if (err) {
      setFormError(err);
      return;
    }
    setFormBusy(true);
    setFormError("");
    try {
      await add(form.vorname, form.nachname, form.email, form.telefon);
      setForm(EMPTY_FORM);
      lastAppliedClipboardRef.current = "";
      setClipboardCustomer(null);
    } catch {
      /* store sets error */
    } finally {
      setFormBusy(false);
    }
  }

  function onFormPaste(e: React.ClipboardEvent) {
    const text = e.clipboardData.getData("text");
    const parsed = parseCustomerJsonPaste(text);
    if (!parsed) return;
    e.preventDefault();
    applyClipboard(parsed);
    lastAppliedClipboardRef.current = text.trim();
    setClipboardCustomer(null);
  }

  async function onAssign(folderPath: string) {
    if (!exportingId) return;
    setAssignBusy(true);
    try {
      await assign(exportingId, folderPath);
      setExportingId(null);
    } catch {
      /* store sets error */
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
      showAppToast("Kunde gelöscht.", { tone: "success" });
    } catch {
      /* store */
    } finally {
      setEditBusy(false);
    }
  }

  const filters: Array<[CustomerFilter, string]> = [
    ["unprocessed", "Offen"],
    ["processed", "Erledigt"],
    ["all", "Alle"],
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-3">
        <div>
          <h2 className="text-sm font-semibold tracking-tight text-foreground sm:text-base">
            Kunden
          </h2>
          <p className="mt-0.5 text-xs text-muted">
            Aufnehmen und per _fertig.txt dem Medienordner zuweisen.
          </p>
        </div>
      </div>

      <Tabs defaultValue="queue" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <div className="shrink-0 px-4 pt-3">
          <TabsList className="w-full sm:w-auto">
            <TabsTrigger value="intake">Aufnehmen</TabsTrigger>
            <TabsTrigger value="queue">Warteschlange</TabsTrigger>
            <TabsTrigger value="history">Zuweisungen</TabsTrigger>
          </TabsList>
        </div>

        {(error || message) && (
          <div className="shrink-0 px-4 pt-2">
            {error ? (
              <p className="text-xs text-destructive">{error}</p>
            ) : (
              <p className="text-xs text-emerald-600 dark:text-emerald-400">{message}</p>
            )}
          </div>
        )}

        <TabsContent
          value="intake"
          className="mt-0 min-h-0 flex-1 overflow-y-auto px-4 py-4 data-[state=inactive]:hidden"
        >
          <form
            onSubmit={(e) => void onSubmit(e)}
            onPaste={onFormPaste}
            className="mx-auto max-w-lg space-y-3"
          >
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

            {formError ? <p className="text-xs text-destructive">{formError}</p> : null}

            <Button type="submit" className="w-full" disabled={formBusy}>
              <UserPlus className="h-3.5 w-3.5" />
              {formBusy ? "Speichern…" : "Kunde anlegen"}
            </Button>
            {(form.vorname || form.nachname || form.email || form.telefon) && (
              <Button
                type="button"
                variant="secondary"
                className="w-full"
                onClick={() => {
                  setForm(EMPTY_FORM);
                  setFormError("");
                  lastAppliedClipboardRef.current = "";
                  void checkClipboard();
                }}
              >
                Zurücksetzen
              </Button>
            )}
          </form>
        </TabsContent>

        <TabsContent
          value="queue"
          className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden px-4 py-3 data-[state=inactive]:hidden"
        >
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <div className="relative min-w-[12rem] flex-1">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <Input
                className="pl-8"
                placeholder="Suchen…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            <div className="flex gap-1">
              {filters.map(([key, label]) => (
                <Button
                  key={key}
                  type="button"
                  size="sm"
                  variant={filter === key ? "default" : "secondary"}
                  onClick={() => setFilter(key)}
                >
                  {label}
                </Button>
              ))}
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border">
            {loading && items.length === 0 ? (
              <p className="p-4 text-sm text-muted">Laden…</p>
            ) : items.length === 0 ? (
              <p className="p-4 text-sm text-muted">Keine Kunden in dieser Ansicht.</p>
            ) : (
              <ul className="divide-y divide-border">
                {items.map((customer) => (
                  <li
                    key={customer.id}
                    className="flex flex-wrap items-center gap-2 px-3 py-2.5 sm:flex-nowrap"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-foreground">
                        {customer.vorname} {customer.nachname}
                        {customer.processed ? (
                          <span className="ml-2 text-[10px] font-normal tracking-wide text-muted uppercase">
                            erledigt
                          </span>
                        ) : null}
                      </p>
                      <p className="truncate text-xs text-muted">
                        {customer.email}
                        {customer.telefon ? ` · ${customer.telefon}` : ""}
                      </p>
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-1.5">
                      <Button
                        type="button"
                        size="sm"
                        disabled={customer.processed}
                        onClick={() => setExportingId(customer.id)}
                        title={
                          customer.processed
                            ? "Bereits zugewiesen"
                            : "Ordner wählen und _fertig.txt schreiben"
                        }
                      >
                        Zuweisen
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        onClick={() => openEdit(customer)}
                        title="Bearbeiten"
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        onClick={() =>
                          void setProcessed(customer.id, !customer.processed)
                        }
                        title={
                          customer.processed
                            ? "Als offen markieren"
                            : "Als erledigt markieren"
                        }
                      >
                        <Check
                          className={cn(
                            "h-3.5 w-3.5",
                            customer.processed && "text-emerald-600",
                          )}
                        />
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </TabsContent>

        <TabsContent
          value="history"
          className="mt-0 min-h-0 flex-1 overflow-y-auto px-4 py-3 data-[state=inactive]:hidden"
        >
          {history.length === 0 ? (
            <p className="text-sm text-muted">Noch keine Zuweisungen.</p>
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
                    <tr key={entry.id}>
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
        </TabsContent>
      </Tabs>

      <FolderSelectionModal
        open={Boolean(exportingId)}
        busy={assignBusy}
        onClose={() => {
          if (!assignBusy) setExportingId(null);
        }}
        onSelect={(path) => void onAssign(path)}
      />

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
              <Button
                type="button"
                disabled={editBusy}
                onClick={() => void saveEdit()}
              >
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
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <Input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
      />
    </div>
  );
}
