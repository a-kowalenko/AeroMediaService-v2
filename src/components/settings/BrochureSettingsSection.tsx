import { useCallback, useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { FileText, Trash2, FolderOpen } from "lucide-react";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  getBrochureStatus,
  importBrochure,
  openBrochure,
  removeBrochure,
  type BrochureStatus,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/store/uiStore";

export type BrochureForm = {
  brochure_enabled: boolean;
  brochure_export_name: string;
  brochure_subdir: string;
};

type Props = {
  active: boolean;
  value: BrochureForm;
  onChange: (next: BrochureForm) => void;
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

export function BrochureSettingsSection({ active, value, onChange }: Props) {
  const showError = useUiStore((s) => s.showError);
  const showSuccess = useUiStore((s) => s.showSuccess);
  const confirm = useUiStore((s) => s.confirm);

  const [status, setStatus] = useState<BrochureStatus | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getBrochureStatus());
    } catch (e) {
      showError(String(e));
    }
  }, [showError]);

  useEffect(() => {
    if (!active) return;
    void refresh();
  }, [active, refresh]);

  const commitPath = useCallback(
    async (path: string) => {
      const trimmed = path.trim();
      if (!trimmed.toLowerCase().endsWith(".pdf")) {
        showError("Nur PDF-Dateien sind erlaubt.");
        return;
      }
      setBusy(true);
      try {
        const next = await importBrochure(trimmed);
        setStatus(next);
        showSuccess("Broschüre gesetzt");
      } catch (e) {
        showError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [showError, showSuccess],
  );

  useEffect(() => {
    if (!active || busy) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          if (busy) {
            setDragOver(false);
            return;
          }
          if (event.payload.type === "enter" || event.payload.type === "over") {
            setDragOver(true);
          } else if (event.payload.type === "leave") {
            setDragOver(false);
          } else if (event.payload.type === "drop") {
            setDragOver(false);
            const pdf = event.payload.paths.find((p) =>
              p.toLowerCase().endsWith(".pdf"),
            );
            if (!pdf) {
              showError("Bitte eine PDF-Datei ablegen.");
              return;
            }
            void commitPath(pdf);
          }
        })
        .then((fn) => {
          if (cancelled) {
            fn();
            return;
          }
          unlisten = fn;
        })
        .catch(() => {
          /* browser preview */
        });
    } catch {
      /* not in Tauri */
    }
    return () => {
      cancelled = true;
      setDragOver(false);
      unlisten?.();
    };
  }, [active, busy, commitPath, showError]);

  async function pickPdf() {
    setBusy(true);
    try {
      const selected = await openFileDialog({
        title: "Infobroschüre-PDF wählen",
        multiple: false,
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      const path =
        typeof selected === "string"
          ? selected
          : Array.isArray(selected)
            ? selected[0]
            : null;
      if (path) await commitPath(path);
    } catch (e) {
      showError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onOpen() {
    try {
      await openBrochure();
    } catch (e) {
      showError(String(e));
    }
  }

  async function onRemove() {
    const ok = await confirm(
      "Die hinterlegte PDF wird aus den App-Daten gelöscht.",
      {
        title: "Infobroschüre entfernen?",
        primaryLabel: "Entfernen",
        secondaryLabel: "Abbrechen",
        destructive: true,
      },
    );
    if (!ok) return;
    setBusy(true);
    try {
      setStatus(await removeBrochure());
      showSuccess("Infobroschüre entfernt");
    } catch (e) {
      showError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const source = status?.source;
  const present = Boolean(source?.present);

  return (
    <SettingsSection
      title="Infobroschüre"
      description="Optional beim Erst-Upload als PDF in den Cloud-Job-Ordner legen (nicht bei Nachreichung)."
    >
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={value.brochure_enabled}
          onCheckedChange={(v) =>
            onChange({ ...value, brochure_enabled: v === true })
          }
        />
        Infobroschüre beim Erst-Upload mitsenden
      </label>

      <div
        className={cn(
          "flex flex-col items-center justify-center gap-2 rounded-md border border-dashed px-3 py-6 text-center text-sm transition-colors",
          dragOver
            ? "border-primary bg-primary/10 text-primary"
            : "border-border bg-muted/20 text-muted",
          busy && "opacity-60",
        )}
      >
        <FileText className="size-6 opacity-70" />
        <p>PDF hierher ziehen (max. 5 MB)</p>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={busy}
          onClick={() => void pickPdf()}
        >
          Datei wählen…
        </Button>
      </div>

      {present ? (
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-border bg-card/50 px-3 py-2 text-sm">
          <div className="min-w-0">
            <p className="truncate font-medium text-foreground">
              {source?.display_name || "Infobroschuere.pdf"}
            </p>
            <p className="text-xs text-muted">
              {formatBytes(source?.size_bytes ?? 0)}
            </p>
          </div>
          <div className="flex shrink-0 gap-1.5">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => void onOpen()}
            >
              <FolderOpen className="size-3.5" />
              Öffnen
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => void onRemove()}
            >
              <Trash2 className="size-3.5" />
              Entfernen
            </Button>
          </div>
        </div>
      ) : (
        <p className="text-xs text-muted">Keine Broschüre hinterlegt.</p>
      )}

      <div className="grid gap-3 sm:grid-cols-2">
        <label className="space-y-1 text-sm">
          <span className="text-xs text-muted">Export-Dateiname</span>
          <Input
            value={value.brochure_export_name}
            placeholder="Infobroschuere.pdf"
            onChange={(e) =>
              onChange({ ...value, brochure_export_name: e.target.value })
            }
          />
        </label>
        <label className="space-y-1 text-sm">
          <span className="text-xs text-muted">
            Unterordner (optional, leer = Job-Root)
          </span>
          <Input
            value={value.brochure_subdir}
            placeholder=""
            onChange={(e) =>
              onChange({ ...value, brochure_subdir: e.target.value })
            }
          />
        </label>
      </div>
    </SettingsSection>
  );
}
