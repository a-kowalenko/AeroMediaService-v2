import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, Folder, ListChecks, Sparkles } from "lucide-react";
import { FolderSelectionModal } from "./FolderSelectionModal";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import {
  proposeCustomerAssignments,
  type Customer,
  type MediaFolderInfo,
} from "@/lib/tauri";
import { useCustomerStore } from "@/store/customerStore";

const NONE = "__none__";

type Row = {
  customer: Customer;
  included: boolean;
  folderPath: string;
  suggestedPath: string;
};

type Props = {
  open: boolean;
  onClose: () => void;
};

function initials(customer: Customer): string {
  const a = customer.vorname.trim().charAt(0);
  const b = customer.nachname.trim().charAt(0);
  const pair = `${a}${b}`.toUpperCase();
  return pair || "?";
}

export function BatchAssignDialog({ open, onClose }: Props) {
  const assignBatch = useCustomerStore((s) => s.assignBatch);
  const [rows, setRows] = useState<Row[]>([]);
  const [folders, setFolders] = useState<MediaFolderInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [pickingId, setPickingId] = useState<string | null>(null);

  const picking = rows.find((r) => r.customer.id === pickingId) ?? null;

  async function loadProposal() {
    setLoading(true);
    setError("");
    try {
      const proposal = await proposeCustomerAssignments();
      setFolders(proposal.folders);
      setRows(
        proposal.rows.map((row) => ({
          customer: row.customer,
          included: row.included && Boolean(row.suggested_path),
          folderPath: row.suggested_path ?? "",
          suggestedPath: row.suggested_path ?? "",
        })),
      );
    } catch (err) {
      setError(String(err));
      setRows([]);
      setFolders([]);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!open) {
      setRows([]);
      setFolders([]);
      setError("");
      setPickingId(null);
      return;
    }
    void loadProposal();
  }, [open]);

  const readyFolders = useMemo(
    () => folders.filter((f) => f.folder_state === "ready"),
    [folders],
  );

  const duplicatePaths = useMemo(() => {
    const counts = new Map<string, number>();
    for (const row of rows) {
      if (!row.included || !row.folderPath) continue;
      counts.set(row.folderPath, (counts.get(row.folderPath) ?? 0) + 1);
    }
    return new Set(
      [...counts.entries()].filter(([, n]) => n > 1).map(([path]) => path),
    );
  }, [rows]);

  const selectedCount = rows.filter((r) => r.included && r.folderPath).length;
  const canConfirm =
    selectedCount > 0 && duplicatePaths.size === 0 && !busy && !loading;

  function updateRow(id: string, patch: Partial<Row>) {
    setRows((current) =>
      current.map((row) => (row.customer.id === id ? { ...row, ...patch } : row)),
    );
  }

  function setFolder(id: string, path: string) {
    updateRow(id, {
      folderPath: path,
      included: Boolean(path),
    });
  }

  function selectMatched() {
    setRows((current) =>
      current.map((row) => ({
        ...row,
        included: Boolean(row.suggestedPath) && row.folderPath === row.suggestedPath,
      })),
    );
  }

  function selectNone() {
    setRows((current) => current.map((row) => ({ ...row, included: false })));
  }

  async function confirm() {
    const items = rows
      .filter((row) => row.included && row.folderPath)
      .map((row) => ({ id: row.customer.id, path: row.folderPath }));
    if (items.length === 0) return;
    setBusy(true);
    try {
      await assignBatch(items);
      onClose();
    } catch {
      /* store toasts */
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Dialog open={open} onOpenChange={(v) => !v && !busy && onClose()}>
        <DialogContent className="flex max-h-[min(90vh,720px)] max-w-4xl flex-col gap-0 overflow-hidden p-0">
          <DialogHeader className="border-b border-border px-5 py-4">
            <DialogTitle>Zuweisungen prüfen</DialogTitle>
            <DialogDescription>
              Offene Kunden den passenden Ordnern zuordnen. Abwählen überspringt,
              Ordner lassen sich noch wechseln.
            </DialogDescription>
          </DialogHeader>

          <div className="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                size="sm"
                variant="secondary"
                disabled={loading || busy}
                onClick={() => selectMatched()}
              >
                Passende anwählen
              </Button>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                disabled={loading || busy}
                onClick={() => selectNone()}
              >
                Alle abwählen
              </Button>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                disabled={loading || busy}
                onClick={() => void loadProposal()}
              >
                Neu vorschlagen
              </Button>
              <p className="ml-auto text-xs text-muted">
                {loading
                  ? "Laden…"
                  : `${selectedCount} von ${rows.length} ausgewählt`}
              </p>
            </div>

            {error ? <p className="text-sm text-destructive">{error}</p> : null}
            {duplicatePaths.size > 0 ? (
              <div className="flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-1.5 text-sm text-destructive">
                <AlertTriangle className="h-4 w-4 shrink-0" />
                <p>Derselbe Ordner ist mehrfach gewählt.</p>
              </div>
            ) : null}

            <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border">
              {rows.length === 0 && !loading ? (
                <p className="p-4 text-sm text-muted">Keine offenen Kunden.</p>
              ) : (
                <ul className="divide-y divide-border">
                  {rows.map((row) => {
                    const duplicate =
                      row.included &&
                      Boolean(row.folderPath) &&
                      duplicatePaths.has(row.folderPath);
                    const recommended =
                      Boolean(row.suggestedPath) &&
                      row.folderPath === row.suggestedPath;
                    return (
                      <li
                        key={row.customer.id}
                        className={cn(
                          "flex flex-col gap-2 px-3 py-2.5 sm:flex-row sm:items-center",
                          duplicate && "bg-destructive/10",
                        )}
                      >
                        <label
                          className={cn(
                            "flex min-w-0 flex-1 items-center gap-3",
                            !row.included && "opacity-60",
                          )}
                        >
                          <Checkbox
                            checked={row.included}
                            disabled={busy}
                            onCheckedChange={(v) =>
                              updateRow(row.customer.id, { included: v === true })
                            }
                            aria-label={`${row.customer.vorname} ${row.customer.nachname} zuweisen`}
                          />
                          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary-soft text-[11px] font-semibold text-primary ring-1 ring-primary/20">
                            {initials(row.customer)}
                          </span>
                          <span className="min-w-0">
                            <span className="block truncate text-sm font-medium text-foreground">
                              {row.customer.vorname}{" "}
                              <span className="font-semibold">{row.customer.nachname}</span>
                            </span>
                            <span className="block truncate text-xs text-muted">
                              {row.customer.email}
                            </span>
                          </span>
                        </label>
                        <div className="flex min-w-0 flex-[1.4] items-center gap-2">
                          <div className="min-w-0 flex-1">
                            <Select
                              value={row.folderPath || NONE}
                              disabled={busy}
                              onValueChange={(value) =>
                                setFolder(row.customer.id, value === NONE ? "" : value)
                              }
                            >
                              <SelectTrigger
                                className="h-8 w-full text-left text-xs"
                                title={row.folderPath || "Kein Ordner"}
                              >
                                <SelectValue placeholder="Ordner wählen" />
                              </SelectTrigger>
                              <SelectContent className="max-h-72 min-w-[min(36rem,calc(100vw-4rem))]">
                                <SelectItem value={NONE}>Kein Ordner</SelectItem>
                                {readyFolders.map((folder) => (
                                  <SelectItem
                                    key={folder.path}
                                    value={folder.path}
                                    title={folder.path}
                                    className="whitespace-normal"
                                  >
                                    {folder.name}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            disabled={busy}
                            title="Anderen Ordner wählen"
                            onClick={() => setPickingId(row.customer.id)}
                          >
                            <Folder className="h-3.5 w-3.5" />
                          </Button>
                          <span className="hidden h-5 w-[5.5rem] shrink-0 items-center justify-center sm:inline-flex">
                            {recommended ? (
                              <span className="inline-flex items-center gap-1 rounded-full border border-amber-400/40 bg-amber-400/15 px-1.5 py-px text-[10px] font-medium tracking-wide text-amber-800 uppercase dark:text-amber-200">
                                <Sparkles className="h-3 w-3" />
                                Treffer
                              </span>
                            ) : duplicate ? (
                              <span className="rounded-full border border-destructive/40 bg-destructive/10 px-1.5 py-px text-[10px] font-medium uppercase tracking-wide text-destructive">
                                doppelt
                              </span>
                            ) : !row.folderPath ? (
                              <span className="text-[11px] text-muted">kein Treffer</span>
                            ) : null}
                          </span>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </div>

          <DialogFooter className="border-t border-border px-5 py-3">
            <Button type="button" variant="secondary" disabled={busy} onClick={onClose}>
              Abbrechen
            </Button>
            <Button type="button" disabled={!canConfirm} onClick={() => void confirm()}>
              <ListChecks className="h-3.5 w-3.5" />
              {busy
                ? "Zuweisen…"
                : selectedCount === 1
                  ? "1 Zuweisung bestätigen"
                  : `${selectedCount} Zuweisungen bestätigen`}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <FolderSelectionModal
        open={Boolean(picking)}
        stacked
        customerLabel={
          picking ? `${picking.customer.vorname} ${picking.customer.nachname}` : undefined
        }
        vorname={picking?.customer.vorname}
        nachname={picking?.customer.nachname}
        email={picking?.customer.email}
        onClose={() => setPickingId(null)}
        onSelect={(path) => {
          if (pickingId) setFolder(pickingId, path);
          setPickingId(null);
        }}
      />
    </>
  );
}
