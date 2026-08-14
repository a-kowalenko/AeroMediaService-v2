import { useEffect, useState } from "react";
import { open as openDirectoryDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Moon, Sun } from "lucide-react";
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
import { getSetting, saveSetting } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useThemeStore, type ThemeMode } from "@/store/themeStore";

const STEPS = ["Darstellung", "Pfade", "Cloud", "Fertig"] as const;
const SKIPPABLE = new Set([0, 1, 2]);

const SKIP_HINT: Record<number, string> = {
  0: "Darstellung kannst du später jederzeit umschalten.",
  1: "Pfade kannst du später in den Einstellungen setzen.",
  2: "Cloud-Dienst und Zugangsdaten später in den Einstellungen.",
};

type Draft = {
  monitor_path: string;
  archive_path: string;
  log_file_path: string;
  selected_cloud_service: string;
};

type Props = {
  open: boolean;
  onComplete: () => void;
};

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

function PathField({
  label,
  value,
  placeholder,
  onChange,
  onPick,
}: {
  label: string;
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
  onPick: () => void;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <div className="flex gap-2">
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
        />
        <Button type="button" variant="secondary" size="icon" onClick={onPick} title="Ordner wählen">
          <FolderOpen className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}

export function SetupWizard({ open, onComplete }: Props) {
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);

  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<Draft>({
    monitor_path: "",
    archive_path: "",
    log_file_path: "",
    selected_cloud_service: "dropbox",
  });
  const [skipped, setSkipped] = useState<Set<number>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void (async () => {
      try {
        const [monitor, archive, log, cloud] = await Promise.all([
          getSetting("monitor_path", ""),
          getSetting("archive_path", ""),
          getSetting("log_file_path", ""),
          getSetting("selected_cloud_service", "dropbox"),
        ]);
        if (cancelled) return;
        setDraft({
          monitor_path: monitor,
          archive_path: archive,
          log_file_path: log,
          selected_cloud_service: cloud || "dropbox",
        });
      } catch {
        /* keep empty draft */
      }
    })();
    setStep(0);
    setSkipped(new Set());
    setError("");
    return () => {
      cancelled = true;
    };
  }, [open]);

  function patch<K extends keyof Draft>(key: K, value: Draft[K]) {
    setDraft((prev) => ({ ...prev, [key]: value }));
  }

  async function persistDraft(markCompleted: boolean) {
    setBusy(true);
    setError("");
    try {
      await saveSetting("monitor_path", draft.monitor_path.trim());
      await saveSetting("archive_path", draft.archive_path.trim());
      await saveSetting("log_file_path", draft.log_file_path.trim());
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
    if (step >= STEPS.length - 1) {
      void persistDraft(true);
      return;
    }
    setStep((s) => s + 1);
  }

  function onBack() {
    setStep((s) => Math.max(0, s - 1));
  }

  async function onSkipAll() {
    await persistDraft(true);
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
            <SettingsSection title="Darstellung" description="Dunkles Design ist der Standard. Hellmodus für helle Umgebungen.">
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
            <SettingsSection title="Ordner">
              <div className="space-y-3">
                <PathField
                  label="Monitor-Pfad"
                  value={draft.monitor_path}
                  placeholder="Überwachter Ordner"
                  onChange={(v) => patch("monitor_path", v)}
                  onPick={() =>
                    void pickDirectory(draft.monitor_path).then((p) => {
                      if (p) patch("monitor_path", p);
                    })
                  }
                />
                <PathField
                  label="Archiv-Pfad"
                  value={draft.archive_path}
                  placeholder="Archiv-Basis"
                  onChange={(v) => patch("archive_path", v)}
                  onPick={() =>
                    void pickDirectory(draft.archive_path).then((p) => {
                      if (p) patch("archive_path", p);
                    })
                  }
                />
                <PathField
                  label="Log-Pfad"
                  value={draft.log_file_path}
                  placeholder="Ordner für Log-Dateien"
                  onChange={(v) => patch("log_file_path", v)}
                  onPick={() =>
                    void pickDirectory(draft.log_file_path).then((p) => {
                      if (p) patch("log_file_path", p);
                    })
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
                  variant={draft.selected_cloud_service !== "custom_api" ? "default" : "secondary"}
                  onClick={() => patch("selected_cloud_service", "dropbox")}
                >
                  Dropbox
                </Button>
                <Button
                  type="button"
                  variant={draft.selected_cloud_service === "custom_api" ? "default" : "secondary"}
                  onClick={() => patch("selected_cloud_service", "custom_api")}
                >
                  Custom API
                </Button>
              </div>
            </SettingsSection>
          ) : null}

          {step === 3 ? (
            <SettingsSection title="Zusammenfassung">
              <dl className="grid gap-2 text-sm">
                {[
                  ["Theme", themeMode === "dark" ? "Dunkel" : "Hell"],
                  ["Monitor", draft.monitor_path.trim() || "— (später)"],
                  ["Archiv", draft.archive_path.trim() || "— (später)"],
                  ["Log", draft.log_file_path.trim() || "— (später)"],
                  [
                    "Cloud",
                    draft.selected_cloud_service === "custom_api" ? "Custom API" : "Dropbox",
                  ],
                ].map(([k, v]) => (
                  <div key={k} className="grid grid-cols-[6.5rem_1fr] gap-2">
                    <dt className="text-muted">{k}</dt>
                    <dd className="break-all text-foreground">{v}</dd>
                  </div>
                ))}
              </dl>
              <p className="mt-3 text-xs text-muted">
                Mit „Fertig“ wird die Einrichtung abgeschlossen. Der Assistent erscheint danach nicht mehr automatisch.
              </p>
            </SettingsSection>
          ) : null}

          {error ? <p className="text-sm text-destructive">{error}</p> : null}
          {SKIPPABLE.has(step) && SKIP_HINT[step] ? (
            <p className="text-xs text-muted">{SKIP_HINT[step]}</p>
          ) : null}
        </div>

        <DialogFooter className="sm:justify-between">
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => void onSkipAll()}
            className="border-warning/40 bg-warning/10 text-foreground hover:bg-warning/20"
          >
            Alles überspringen
          </Button>
          <div className="flex flex-wrap justify-end gap-2">
            <Button type="button" variant="secondary" disabled={busy || step === 0} onClick={onBack}>
              Zurück
            </Button>
            {SKIPPABLE.has(step) ? (
              <Button type="button" variant="secondary" disabled={busy} onClick={onSkipStep}>
                Schritt überspringen
              </Button>
            ) : null}
            <Button type="button" disabled={busy} onClick={onNext}>
              {step >= STEPS.length - 1 ? (busy ? "Speichern…" : "Fertig") : "Weiter"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
