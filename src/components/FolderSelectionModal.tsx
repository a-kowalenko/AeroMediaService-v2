import { useEffect, useState } from "react";
import { ArrowUp, Folder, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import {
  listMediaFolders,
  type MediaFolderInfo,
} from "@/lib/tauri";

type Props = {
  open: boolean;
  onClose: () => void;
  onSelect: (folderPath: string) => void;
  busy?: boolean;
  title?: string;
};

function rowClass(folder: MediaFolderInfo): string {
  if (!folder.is_ready || folder.folder_state === "busy") {
    return "border-destructive/30 bg-destructive/10 hover:bg-destructive/15";
  }
  if (folder.folder_state === "occupied" || folder.block_reason) {
    return "border-sky-500/30 bg-sky-500/10";
  }
  return "border-emerald-500/30 bg-emerald-500/10 hover:bg-emerald-500/15";
}

function iconClass(folder: MediaFolderInfo): string {
  if (!folder.is_ready || folder.folder_state === "busy") return "text-destructive";
  if (folder.folder_state === "occupied" || folder.block_reason) return "text-sky-600 dark:text-sky-400";
  return "text-emerald-600 dark:text-emerald-400";
}

export function FolderSelectionModal({
  open,
  onClose,
  onSelect,
  busy = false,
  title = "Zielordner wählen",
}: Props) {
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [parentPath, setParentPath] = useState<string | null>(null);
  const [folders, setFolders] = useState<MediaFolderInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function loadDirectory(path: string | null = null) {
    setLoading(true);
    setError("");
    try {
      const res = await listMediaFolders(path);
      setFolders(res.folders);
      setCurrentPath(res.path);
      setParentPath(res.parent);
    } catch (err) {
      setError(String(err));
      setFolders([]);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!open) return;
    void loadDirectory(null);
  }, [open]);

  useEffect(() => {
    if (!open || !currentPath) return;
    const interval = window.setInterval(() => {
      void loadDirectory(currentPath);
    }, 2000);
    return () => window.clearInterval(interval);
  }, [open, currentPath]);

  function isBlocked(folder: MediaFolderInfo): boolean {
    return folder.folder_state === "occupied" || Boolean(folder.block_reason);
  }

  function selectTitle(folder: MediaFolderInfo): string {
    if (busy) return "Zuweisung läuft…";
    if (!folder.is_ready) return "Ordner wird noch beschrieben";
    if (folder.block_reason) return `Belegt durch ${folder.block_reason}`;
    return "In diesen Ordner zuweisen";
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="flex max-h-[min(90vh,640px)] max-w-2xl flex-col gap-0 overflow-hidden p-0">
        <DialogHeader className="border-b border-border px-5 py-4">
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            Unterordner des Überwachungspfads. Grün = bereit, Blau = Marker vorhanden,
            Rot = Schreibvorgang aktiv.
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
          <div className="flex items-center gap-2">
            <p className="min-w-0 flex-1 truncate rounded-md bg-card-elevated px-3 py-2 font-mono text-xs text-muted">
              {currentPath ?? "…"}
            </p>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={loading || !parentPath || parentPath === currentPath}
              onClick={() => parentPath && void loadDirectory(parentPath)}
              title="Übergeordneter Ordner"
            >
              <ArrowUp className="h-3.5 w-3.5" />
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={loading}
              onClick={() => void loadDirectory(currentPath)}
              title="Aktualisieren"
            >
              <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
            </Button>
          </div>

          {error ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : null}

          <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border">
            {folders.length === 0 && !loading ? (
              <p className="p-4 text-sm text-muted">Keine Unterordner gefunden.</p>
            ) : (
              <ul className="divide-y divide-border">
                {folders.map((folder) => {
                  const blocked = isBlocked(folder);
                  const disabled =
                    busy || !folder.is_ready || blocked || folder.folder_state === "busy";
                  return (
                    <li key={folder.path}>
                      <div
                        className={cn(
                          "flex items-center gap-3 px-3 py-2.5 transition-colors",
                          rowClass(folder),
                        )}
                      >
                        <button
                          type="button"
                          className="flex min-w-0 flex-1 items-center gap-2 text-left"
                          onClick={() => void loadDirectory(folder.path)}
                          title="Ordner öffnen"
                        >
                          <Folder className={cn("h-4 w-4 shrink-0", iconClass(folder))} />
                          <span className="truncate text-sm font-medium text-foreground">
                            {folder.name}
                          </span>
                          {folder.block_reason ? (
                            <span className="truncate text-xs text-muted">
                              {folder.block_reason}
                            </span>
                          ) : null}
                        </button>
                        <Button
                          type="button"
                          size="sm"
                          disabled={disabled}
                          title={selectTitle(folder)}
                          onClick={() => onSelect(folder.path)}
                        >
                          Zuweisen
                        </Button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
