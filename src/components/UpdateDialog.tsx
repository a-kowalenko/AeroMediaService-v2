import { useEffect, useState } from "react";
import { ChevronDown } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { ProgressBar } from "./ProgressBar";
import { ReleaseNotes } from "@/components/ReleaseNotes";
import { cn } from "@/lib/utils";
import type { UpdateInstallProgress } from "@/lib/tauri";
import { compareVersionParts } from "@/lib/versionCompare";

export type UpdateDialogProps = {
  open: boolean;
  fromVersion: string;
  toVersion: string | null;
  notes: string | null;
  available: boolean;
  message: string;
  installing?: boolean;
  installProgress?: UpdateInstallProgress | null;
  silentAvailable?: boolean;
  blockedReason?: string | null;
  platformHint?: string | null;
  installerUrl?: string | null;
  allowIgnore?: boolean;
  onInstall: () => void;
  onCancelInstall?: () => void;
  onLater: (ignoreVersion: boolean) => void;
  onClose: (ignoreVersion: boolean) => void;
};

function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${Math.round(n)} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return "";
  return `${formatBytes(bps)}/s`;
}

function installDirection(
  from: string,
  to: string | null,
): "upgrade" | "downgrade" | "same" | null {
  if (!to) return null;
  const cmp = compareVersionParts(to, from);
  if (cmp > 0) return "upgrade";
  if (cmp < 0) return "downgrade";
  return "same";
}

export function UpdateDialog({
  open,
  fromVersion,
  toVersion,
  notes,
  available,
  message,
  installing = false,
  installProgress = null,
  silentAvailable = true,
  blockedReason = null,
  platformHint = null,
  installerUrl = null,
  allowIgnore = false,
  onInstall,
  onCancelInstall,
  onLater,
  onClose,
}: UpdateDialogProps) {
  const [notesOpen, setNotesOpen] = useState(false);
  const [ignoreVersion, setIgnoreVersion] = useState(false);
  const direction = installDirection(fromVersion, toVersion);
  const isDowngrade = direction === "downgrade";
  const canSilentInstall =
    available &&
    silentAvailable &&
    Boolean(toVersion) &&
    direction !== "same";
  const installDisabled = installing || Boolean(blockedReason) || !canSilentInstall;
  const phase = installProgress?.phase ?? (installing ? "download" : null);
  const canCancelDownload =
    installing && phase !== "install" && Boolean(onCancelInstall);
  const showIgnore = allowIgnore && available && !isDowngrade && !installing;

  useEffect(() => {
    if (!open) {
      setNotesOpen(false);
      setIgnoreVersion(false);
    }
  }, [open]);

  const progressLabel =
    phase === "install"
      ? isDowngrade
        ? "Version wird installiert…"
        : "Update wird installiert…"
      : phase === "download"
        ? isDowngrade
          ? "Version wird heruntergeladen…"
          : "Update wird heruntergeladen…"
        : installing
          ? "Installation wird vorbereitet…"
          : undefined;

  const detailParts: string[] = [];
  if (installProgress && phase === "download") {
    const done = formatBytes(installProgress.downloadedBytes);
    const total =
      installProgress.totalBytes != null && installProgress.totalBytes > 0
        ? formatBytes(installProgress.totalBytes)
        : null;
    detailParts.push(total ? `${done} / ${total}` : done);
    const speed = formatSpeed(installProgress.speedBps);
    if (speed) detailParts.push(speed);
  }

  const title = !available
    ? "Update-Prüfung"
    : isDowngrade
      ? "Ältere Version installieren"
      : "Update verfügbar";

  const primaryLabel = installing
    ? "Installiere…"
    : isDowngrade
      ? "Jetzt wechseln"
      : "Jetzt aktualisieren";

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !installing) onClose(ignoreVersion);
      }}
    >
      <DialogContent
        className="z-[70] max-w-lg overflow-hidden"
        overlayClassName="z-[70]"
        hideCloseButton={installing}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          const root = e.currentTarget;
          if (!(root instanceof HTMLElement)) return;
          const primary =
            root.querySelector<HTMLElement>("[data-update-primary]") ??
            root.querySelector<HTMLElement>("button:not([disabled])");
          primary?.focus();
        }}
        onEscapeKeyDown={(e) => {
          if (canCancelDownload) {
            e.preventDefault();
            onCancelInstall?.();
            return;
          }
          if (installing) e.preventDefault();
        }}
        onPointerDownOutside={(e) => {
          if (installing) e.preventDefault();
        }}
        onInteractOutside={(e) => {
          if (installing) e.preventDefault();
        }}
      >
        <DialogHeader className="min-w-0">
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription className="min-w-0 break-words">
            {message || "Update-Status wird geladen…"}
          </DialogDescription>
        </DialogHeader>

        {available && toVersion ? (
          <div className="min-w-0 space-y-3 text-sm">
            <p className="min-w-0 break-words">
              {isDowngrade ? (
                <>
                  Version <strong>{toVersion}</strong> ersetzt die aktuelle{" "}
                  <strong>{fromVersion}</strong>.
                </>
              ) : (
                <>
                  Version <strong>{toVersion}</strong> kann installiert werden.
                  <br />
                  Aktuell: {fromVersion}
                </>
              )}
            </p>
            {isDowngrade ? (
              <p className="text-xs text-muted">
                Die App wird ersetzt und neu gestartet. Einstellungen bleiben in
                der Regel erhalten.
              </p>
            ) : null}
            <div className="min-w-0 space-y-1.5">
              <button
                type="button"
                className="inline-flex items-center gap-1 rounded-sm text-xs font-medium text-muted transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                aria-expanded={notesOpen}
                disabled={installing}
                onClick={() => setNotesOpen((v) => !v)}
              >
                Patchnotes
                <ChevronDown
                  className={cn(
                    "h-3.5 w-3.5 transition-transform duration-200",
                    notesOpen && "rotate-180",
                  )}
                />
              </button>
              {notesOpen ? (
                <div className="min-w-0 overflow-hidden border-l border-border/70 pl-3">
                  <ReleaseNotes
                    markdown={notes?.trim() ?? ""}
                    emptyLabel="Keine Patchnotes verfügbar."
                    className="max-h-48"
                  />
                </div>
              ) : null}
            </div>
          </div>
        ) : null}

        {platformHint && !installing ? (
          <p className="text-xs text-muted">{platformHint}</p>
        ) : null}

        {blockedReason && !installing ? (
          <p className="text-xs text-destructive">{blockedReason}</p>
        ) : null}

        {showIgnore ? (
          <div className="flex items-center gap-2">
            <Checkbox
              id="ignore-version"
              checked={ignoreVersion}
              onCheckedChange={(v) => setIgnoreVersion(v === true)}
            />
            <Label htmlFor="ignore-version" className="text-sm font-normal text-muted">
              Diese Version nicht mehr anzeigen
            </Label>
          </div>
        ) : null}

        {installing ? (
          <div className="min-w-0 space-y-2">
            <ProgressBar
              percent={
                installProgress?.percent ?? (phase === "install" ? 100 : 0)
              }
              label={progressLabel}
            />
            {detailParts.length > 0 ? (
              <p className="text-xs tabular-nums text-muted">
                {detailParts.join(" · ")}
              </p>
            ) : null}
          </div>
        ) : null}

        <DialogFooter className="min-w-0 gap-2">
          {available ? (
            <>
              {canCancelDownload ? (
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() => onCancelInstall?.()}
                >
                  Abbrechen
                </Button>
              ) : (
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() => onLater(ignoreVersion)}
                  disabled={installing}
                >
                  Später
                </Button>
              )}
              {canSilentInstall ? (
                <Button
                  type="button"
                  data-update-primary
                  onClick={onInstall}
                  disabled={installDisabled}
                >
                  {primaryLabel}
                </Button>
              ) : installerUrl ? (
                <Button
                  type="button"
                  variant="secondary"
                  data-update-primary
                  disabled={installing}
                  onClick={() => {
                    void openUrl(installerUrl).catch(() => undefined);
                  }}
                >
                  Installer herunterladen
                </Button>
              ) : null}
            </>
          ) : (
            <Button
              type="button"
              data-update-primary
              onClick={() => onClose(ignoreVersion)}
            >
              OK
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
