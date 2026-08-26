import { useEffect, useState } from "react";
import { open as openDirectoryDialog } from "@tauri-apps/plugin-dialog";
import { Check, FolderOpen, Info, Loader2, Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { applyDefaultAppRoot, inferAppRoot } from "@/lib/defaultDirs";
import {
  ensureDefaultAppRoot,
  getSetting,
  proposeDefaultDirs,
  saveSetting,
  type DefaultDirsProposal,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useThemeStore, type ThemeMode } from "@/store/themeStore";

const STEPS = ["Darstellung", "Pfade", "Cloud", "Fertig"] as const;
/** Appearance + Cloud only — paths require a monitor folder. */
const SKIPPABLE = new Set([0, 2]);

const SKIP_HINT: Record<number, string> = {
  0: "Darstellung kannst du später jederzeit umschalten.",
  2: "Cloud-Dienst und Zugangsdaten später in den Einstellungen.",
};

type Draft = {
  monitor_path: string;
  app_root: string;
  archive_path: string;
  log_file_path: string;
  selected_cloud_service: string;
};

type Props = {
  open: boolean;
  onComplete: () => void;
};

function pathsEqual(a: string, b: string): boolean {
  const norm = (p: string) =>
    p
      .trim()
      .replace(/[/\\]+$/, "")
      .replace(/\\/g, "/")
      .toLowerCase();
  return norm(a) === norm(b);
}

async function pickDirectory(current: string): Promise<string | null> {
  try {
    const selected = await openDirectoryDialog({
      directory: true,
      multiple: false,
      defaultPath: current || undefined,
      title: "Ordner auswählen",
    });
    if (typeof selected === "string") return selected;
    return null;
  } catch {
    return null;
  }
}

function StandardDirButton({
  label,
  busy,
  lockedDone,
  tone,
  disabled,
  onClick,
}: {
  label: string;
  busy: boolean;
  lockedDone: boolean;
  tone?: "adopt" | "adopted";
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      size="sm"
      variant={tone === "adopted" ? "default" : "secondary"}
      disabled={disabled || busy || lockedDone}
      onClick={onClick}
      className={cn(
        "shrink-0 gap-1.5",
        tone === "adopted" && "bg-emerald-600 hover:bg-emerald-600",
      )}
    >
      {busy ? (
        <Loader2 className="size-3.5 shrink-0 animate-spin" aria-hidden />
      ) : lockedDone || tone === "adopted" ? (
        <Check className="size-3.5 shrink-0" strokeWidth={2.5} aria-hidden />
      ) : null}
      {label}
    </Button>
  );
}

function FolderDirField({
  label,
  value,
  placeholder,
  standardPath,
  standardExists,
  required,
  invalid,
  onPick,
  busy,
  done,
  createDisabled,
  onCreate,
  onUseStandard,
  error,
  hint,
}: {
  label: string;
  value: string;
  placeholder: string;
  standardPath?: string | null;
  standardExists?: boolean;
  required?: boolean;
  invalid?: boolean;
  onPick: () => void;
  busy: boolean;
  done: boolean;
  createDisabled?: boolean;
  onCreate?: () => void;
  onUseStandard?: () => void;
  error?: string;
  hint?: string;
}) {
  const usingStandard = Boolean(standardPath && pathsEqual(value, standardPath));
  const alreadyOnDisk = Boolean(standardExists) || done;
  const showExistsStrip = Boolean(standardPath) && alreadyOnDisk && onUseStandard;
  const showCreateStrip = Boolean(standardPath) && !alreadyOnDisk && onCreate;

  return (
    <div className="space-y-1.5">
      <Label>
        {label}
        {required ? <span className="text-destructive"> *</span> : null}
      </Label>
      <div className="relative">
        <Input
          value={value}
          readOnly
          placeholder={placeholder}
          aria-invalid={invalid || undefined}
          className={cn(
            "pr-9",
            invalid && "border-destructive focus-visible:ring-destructive/40",
            usingStandard &&
              !invalid &&
              "border-emerald-500/40 focus-visible:ring-emerald-500/25",
          )}
        />
        <button
          type="button"
          onClick={onPick}
          title="Ordner wählen"
          aria-label="Ordner wählen"
          className={cn(
            "absolute top-1/2 right-1 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded text-muted transition-colors",
            "hover:bg-primary-soft hover:text-foreground",
          )}
        >
          <FolderOpen className="h-3.5 w-3.5" aria-hidden />
        </button>
      </div>
      {hint ? <p className="text-[11px] leading-snug text-muted">{hint}</p> : null}
      {standardPath && showExistsStrip ? (
        <div
          className={cn(
            "flex items-center gap-2.5 rounded-md border px-2.5 py-2 transition-colors duration-300",
            usingStandard
              ? "border-emerald-500/35 bg-emerald-500/12 dark:border-emerald-400/30 dark:bg-emerald-400/10"
              : "border-amber-400/35 bg-amber-500/[0.08] dark:border-amber-400/30 dark:bg-amber-400/[0.08]",
          )}
        >
          <span
            className={cn(
              "flex size-7 shrink-0 items-center justify-center rounded-full",
              usingStandard
                ? "bg-emerald-500/20 text-emerald-800 dark:text-emerald-100"
                : "bg-amber-500/15 text-amber-800 dark:text-amber-100",
            )}
            aria-hidden
          >
            {usingStandard ? (
              <Check className="size-3.5" strokeWidth={2.5} />
            ) : (
              <Info className="size-3.5" strokeWidth={2.5} />
            )}
          </span>
          <div className="min-w-0 flex-1">
            <p
              className={cn(
                "text-xs font-medium",
                usingStandard
                  ? "text-emerald-950 dark:text-emerald-50"
                  : "text-amber-950 dark:text-amber-50",
              )}
            >
              {usingStandard ? "Standardordner aktiv" : "Standardordner vorhanden"}
            </p>
            <p className="truncate text-[11px] text-muted" title={standardPath}>
              {standardPath}
            </p>
          </div>
          <StandardDirButton
            busy={busy}
            lockedDone={usingStandard}
            tone={usingStandard ? "adopted" : "adopt"}
            label={usingStandard ? "Aktiv" : "Übernehmen"}
            disabled={createDisabled}
            onClick={() => onUseStandard?.()}
          />
        </div>
      ) : showCreateStrip ? (
        <div className="flex items-center gap-2">
          <p
            className="min-w-0 flex-1 truncate text-xs text-muted"
            title={standardPath ?? undefined}
          >
            Standard: {standardPath}
          </p>
          <StandardDirButton
            busy={busy}
            lockedDone={false}
            label="Anlegen"
            disabled={createDisabled}
            onClick={() => onCreate?.()}
          />
        </div>
      ) : null}
      {error ? (
        <p className="text-[11px] leading-snug text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export function SetupWizard({ open, onComplete }: Props) {
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);

  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<Draft>({
    monitor_path: "",
    app_root: "",
    archive_path: "",
    log_file_path: "",
    selected_cloud_service: "dropbox",
  });
  const [proposal, setProposal] = useState<DefaultDirsProposal | null>(null);
  const [appRootDone, setAppRootDone] = useState(false);
  const [creatingRoot, setCreatingRoot] = useState(false);
  const [skipped, setSkipped] = useState<Set<number>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [fieldErrors, setFieldErrors] = useState<{ monitor_path?: string }>({});

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void (async () => {
      try {
        const [monitor, archive, log, cloud, dirs] = await Promise.all([
          getSetting("monitor_path", ""),
          getSetting("archive_path", ""),
          getSetting("log_file_path", ""),
          getSetting("selected_cloud_service", "dropbox"),
          proposeDefaultDirs().catch(() => null),
        ]);
        if (cancelled) return;
        const appRoot = inferAppRoot(archive, log, dirs?.root);
        setDraft({
          monitor_path: monitor,
          app_root: appRoot,
          archive_path: archive,
          log_file_path: log,
          selected_cloud_service: cloud || "dropbox",
        });
        setProposal(dirs);
        setAppRootDone(
          Boolean(
            appRoot.trim() &&
              dirs &&
              pathsEqual(appRoot, dirs.root) &&
              (dirs.root_exists || (dirs.archive_exists && dirs.log_exists)),
          ),
        );
      } catch {
        /* keep empty draft */
      }
    })();
    setStep(0);
    setSkipped(new Set());
    setError("");
    setFieldErrors({});
    setCreatingRoot(false);
    return () => {
      cancelled = true;
    };
  }, [open]);

  function patch<K extends keyof Draft>(key: K, value: Draft[K]) {
    setDraft((prev) => ({ ...prev, [key]: value }));
  }

  function applyAppRootPaths(root: string, archive: string, log: string) {
    setDraft((prev) => ({
      ...prev,
      app_root: root,
      archive_path: archive,
      log_file_path: log,
    }));
  }

  function validateMonitor(): boolean {
    if (draft.monitor_path.trim()) {
      setFieldErrors({});
      return true;
    }
    setFieldErrors({
      monitor_path: "Bitte den Monitor-Ordner wählen (z. B. SMB-Share „aktuell“).",
    });
    return false;
  }

  async function persistDraft(markCompleted: boolean) {
    if (!validateMonitor()) {
      setStep(1);
      return;
    }
    setBusy(true);
    setError("");
    try {
      let archive = draft.archive_path.trim();
      let log = draft.log_file_path.trim();
      const root = draft.app_root.trim();
      if (root && (!archive || !log)) {
        const ensured = await ensureDefaultAppRoot(root);
        archive = ensured.archive_path;
        log = ensured.log_path;
        applyAppRootPaths(ensured.root, archive, log);
      }
      await saveSetting("monitor_path", draft.monitor_path.trim());
      await saveSetting("archive_path", archive);
      await saveSetting("log_file_path", log);
      await saveSetting(
        "selected_cloud_service",
        draft.selected_cloud_service === "custom_api" ? "custom_api" : "dropbox",
      );
      await saveSetting("ui_theme", themeMode);
      if (markCompleted) {
        await saveSetting("setup_completed", "true");
      }
      onComplete();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function onSkipStep() {
    if (!SKIPPABLE.has(step)) return;
    setSkipped((prev) => new Set(prev).add(step));
    setStep((s) => Math.min(s + 1, STEPS.length - 1));
  }

  function onNext() {
    if (step === 1 && !validateMonitor()) return;
    if (step >= STEPS.length - 1) {
      void persistDraft(true);
      return;
    }
    setFieldErrors({});
    setSkipped((prev) => {
      if (!prev.has(step)) return prev;
      const next = new Set(prev);
      next.delete(step);
      return next;
    });
    setStep((s) => s + 1);
  }

  function onBack() {
    setFieldErrors({});
    setStep((s) => Math.max(0, s - 1));
  }

  async function onSkipAll() {
    await persistDraft(true);
  }

  async function onCreateStandardRoot() {
    setCreatingRoot(true);
    setError("");
    try {
      const result = await applyDefaultAppRoot();
      if (!result) return;
      applyAppRootPaths(
        result.ensured.root,
        result.ensured.archive_path,
        result.ensured.log_path,
      );
      setAppRootDone(true);
      try {
        setProposal(await proposeDefaultDirs());
      } catch {
        /* keep previous proposal */
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setCreatingRoot(false);
    }
  }

  async function onUseStandardRoot() {
    if (!proposal) return;
    setCreatingRoot(true);
    setError("");
    try {
      // Ensure Archiv/Logs (and status folders) exist under the standard root.
      const ensured = await ensureDefaultAppRoot(proposal.root);
      applyAppRootPaths(ensured.root, ensured.archive_path, ensured.log_path);
      setAppRootDone(true);
      setProposal(await proposeDefaultDirs().catch(() => proposal));
    } catch (err) {
      setError(String(err));
    } finally {
      setCreatingRoot(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={() => { /* setup required */ }}>
      <DialogContent
        className="z-[120] max-w-xl overflow-hidden"
        overlayClassName="z-[120]"
        hideCloseButton
        onPointerDownOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <p className="text-[10px] font-semibold tracking-[0.14em] text-primary uppercase">
            Willkommen
          </p>
          <DialogTitle>Aero Media Service einrichten</DialogTitle>
          <DialogDescription>
            Kurze Einrichtung der Kernpfade und des Cloud-Dienstes. Alles später änderbar.
          </DialogDescription>
        </DialogHeader>

        <ol className="flex flex-wrap gap-1.5">
          {STEPS.map((label, i) => {
            const active = i === step;
            const done = i < step || skipped.has(i);
            return (
              <li
                key={label}
                className={cn(
                  "rounded-full border px-2.5 py-1 text-[11px] font-medium",
                  active && "border-primary/40 bg-primary-soft text-foreground",
                  done && !active && "border-border text-primary",
                  !active && !done && "border-border text-muted",
                )}
              >
                {label}
              </li>
            );
          })}
        </ol>

        <div className="min-h-[12rem] space-y-3">
          {step === 0 ? (
            <SettingsSection
              title="Darstellung"
              description="Dunkles Design ist der Standard. Hellmodus für helle Umgebungen."
            >
              <div className="flex flex-wrap gap-2">
                {(["dark", "light"] as ThemeMode[]).map((mode) => (
                  <Button
                    key={mode}
                    type="button"
                    variant={themeMode === mode ? "default" : "secondary"}
                    onClick={() => setThemeMode(mode)}
                  >
                    {mode === "dark" ? (
                      <Moon className="h-4 w-4" />
                    ) : (
                      <Sun className="h-4 w-4" />
                    )}
                    {mode === "dark" ? "Dunkel" : "Hell"}
                  </Button>
                ))}
              </div>
            </SettingsSection>
          ) : null}

          {step === 1 ? (
            <SettingsSection
              title="Ordner"
              description="Monitor-Pfad ist Pflicht. Unter AeroMediaService werden Archiv und Logs angelegt."
            >
              <div className="space-y-3">
                <FolderDirField
                  label="Monitor-Pfad"
                  value={draft.monitor_path}
                  placeholder="Überwachter Ordner wählen"
                  required
                  invalid={Boolean(fieldErrors.monitor_path)}
                  error={fieldErrors.monitor_path}
                  hint='Von hier aus werden die Medien hochgeladen (z. B. „Aktuell“-Ordner).'
                  busy={false}
                  done={false}
                  onPick={() =>
                    void pickDirectory(draft.monitor_path).then((p) => {
                      if (p) {
                        patch("monitor_path", p);
                        setFieldErrors({});
                      }
                    })
                  }
                />
                <FolderDirField
                  label="AeroMediaService"
                  value={draft.app_root}
                  placeholder="App-Ordner (enthält Archiv und Logs)"
                  standardPath={proposal?.root}
                  standardExists={
                    Boolean(proposal?.root_exists) ||
                    Boolean(proposal?.archive_exists && proposal?.log_exists)
                  }
                  busy={creatingRoot}
                  done={appRootDone}
                  createDisabled={busy || creatingRoot}
                  hint="Darunter entstehen Archiv (1 Erfolgreich / 2 Abgebrochen / 3 Fehler) und Logs."
                  onCreate={() => void onCreateStandardRoot()}
                  onUseStandard={() => void onUseStandardRoot()}
                  onPick={() =>
                    void (async () => {
                      const p = await pickDirectory(
                        draft.app_root || proposal?.root || "",
                      );
                      if (!p) return;
                      setCreatingRoot(true);
                      setError("");
                      try {
                        const ensured = await ensureDefaultAppRoot(p);
                        applyAppRootPaths(
                          ensured.root,
                          ensured.archive_path,
                          ensured.log_path,
                        );
                        setAppRootDone(
                          Boolean(
                            proposal && pathsEqual(ensured.root, proposal.root),
                          ),
                        );
                      } catch (err) {
                        setError(String(err));
                      } finally {
                        setCreatingRoot(false);
                      }
                    })()
                  }
                />
              </div>
            </SettingsSection>
          ) : null}

          {step === 2 ? (
            <SettingsSection
              title="Cloud-Dienst"
              description="Zugangsdaten trägst du danach in den Einstellungen ein — oder sie werden aus der Legacy-Installation übernommen."
            >
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant={
                    draft.selected_cloud_service !== "custom_api" ? "default" : "secondary"
                  }
                  onClick={() => patch("selected_cloud_service", "dropbox")}
                >
                  Dropbox
                </Button>
                <Button
                  type="button"
                  variant={
                    draft.selected_cloud_service === "custom_api" ? "default" : "secondary"
                  }
                  onClick={() => patch("selected_cloud_service", "custom_api")}
                >
                  Skydive Media
                </Button>
              </div>
            </SettingsSection>
          ) : null}

          {step === 3 ? (
            <SettingsSection title="Zusammenfassung">
              <dl className="grid gap-2 text-sm">
                {[
                  ["Theme", themeMode === "dark" ? "Dunkel" : "Hell"],
                  ["Monitor", draft.monitor_path.trim() || "— fehlt"],
                  ["App-Ordner", draft.app_root.trim() || "— (später)"],
                  [
                    "Cloud",
                    draft.selected_cloud_service === "custom_api"
                      ? "Skydive Media"
                      : "Dropbox",
                  ],
                ].map(([k, v]) => (
                  <div key={k} className="grid grid-cols-[6.5rem_1fr] gap-2">
                    <dt className="text-muted">{k}</dt>
                    <dd
                      className={cn(
                        "break-all text-foreground",
                        k === "Monitor" && !draft.monitor_path.trim() && "text-destructive",
                      )}
                    >
                      {v}
                    </dd>
                  </div>
                ))}
              </dl>
              <p className="mt-3 text-xs text-muted">
                Mit „Fertig“ wird die Einrichtung abgeschlossen. Der Assistent erscheint danach
                nicht mehr automatisch.
              </p>
            </SettingsSection>
          ) : null}

          {error ? <p className="text-sm text-destructive">{error}</p> : null}
          {SKIPPABLE.has(step) && SKIP_HINT[step] ? (
            <p className="text-xs text-muted">{SKIP_HINT[step]}</p>
          ) : null}
          {step === 1 ? (
            <p className="text-xs text-muted">
              Monitor-Pfad ist erforderlich. Der AeroMediaService-Ordner ist optional und später in
              den Einstellungen änderbar.
            </p>
          ) : null}
        </div>

        <DialogFooter className="sm:justify-between">
          <Button
            type="button"
            variant="outline"
            disabled={busy || creatingRoot}
            onClick={() => void onSkipAll()}
            className="border-warning/40 bg-warning/10 text-foreground hover:bg-warning/20"
            title="Darstellung und Cloud überspringen — Monitor-Pfad bleibt Pflicht"
          >
            Alles überspringen
          </Button>
          <div className="flex flex-wrap justify-end gap-2">
            <Button
              type="button"
              variant="secondary"
              disabled={busy || creatingRoot || step === 0}
              onClick={onBack}
            >
              Zurück
            </Button>
            {SKIPPABLE.has(step) ? (
              <Button
                type="button"
                variant="secondary"
                disabled={busy || creatingRoot}
                onClick={onSkipStep}
              >
                Schritt überspringen
              </Button>
            ) : null}
            <Button type="button" disabled={busy || creatingRoot} onClick={onNext}>
              {step >= STEPS.length - 1 ? (busy ? "Speichern…" : "Fertig") : "Weiter"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
