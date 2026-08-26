import { useCallback, useEffect, useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Spinner } from "@/components/Spinner";
import { ReleaseNotes } from "@/components/ReleaseNotes";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  getSetting,
  getUpdaterStatus,
  listAvailableVersions,
  migrateLegacySettings,
  resetSetup,
  saveSetting,
  type AvailableRelease,
  type MigrateReport,
} from "@/lib/tauri";
import { compareVersionParts, isVersionPrerelease } from "@/lib/versionCompare";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/store/uiStore";

type Props = {
  active: boolean;
  appVersion?: string;
  platformHint?: string | null;
  installBlockedReason?: string | null;
  onRequestUpdateCheck?: (includeBeta?: boolean) => void;
  onRequestVersionSwitch?: (release: AvailableRelease) => void;
  onOpenSetupWizard?: () => void;
  onCloseSettings: () => void;
};

function formatReleaseDate(iso: string): string {
  const trimmed = iso.trim();
  if (!trimmed) return "";
  const date = new Date(trimmed);
  if (Number.isNaN(date.getTime())) return trimmed;
  return new Intl.DateTimeFormat("de-DE", {
    dateStyle: "medium",
  }).format(date);
}

type VersionRelation = "newer" | "older" | "same" | null;

type MetaTone = "neutral" | "active" | "warning" | "muted";

function MetaChip({ label, tone = "neutral" }: { label: string; tone?: MetaTone }) {
  const toneClass =
    tone === "active"
      ? "border-success/40 bg-success/10 text-success"
      : tone === "warning"
        ? "border-warning/45 bg-warning/10 text-warning"
        : tone === "muted"
          ? "border-border/70 bg-muted/30 text-muted"
          : "border-primary/40 bg-primary/10 text-primary";
  return (
    <span
      className={cn(
        "inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none",
        toneClass,
      )}
    >
      {label}
    </span>
  );
}

function releaseMeta(
  release: AvailableRelease,
  index: number,
  appVersion?: string,
): { label: string; tone: MetaTone }[] {
  const chips: { label: string; tone: MetaTone }[] = [];
  if (index === 0) chips.push({ label: "Neueste", tone: "active" });
  if (appVersion && compareVersionParts(release.tag_name, appVersion) === 0) {
    chips.push({ label: "Installiert", tone: "neutral" });
  }
  if (release.prerelease) chips.push({ label: "Beta", tone: "warning" });
  if (!release.updater_json_url) {
    chips.push({ label: "Manuell", tone: "muted" });
  }
  return chips;
}

export function ExtrasSettingsSection({
  active,
  appVersion,
  platformHint,
  installBlockedReason = null,
  onRequestUpdateCheck,
  onRequestVersionSwitch,
  onOpenSetupWizard,
  onCloseSettings,
}: Props) {
  const showError = useUiStore((s) => s.showError);
  const showSuccess = useUiStore((s) => s.showSuccess);
  const confirm = useUiStore((s) => s.confirm);

  /** Only set when auto-update is unavailable — omit in the happy path. */
  const [updaterUnavailableMessage, setUpdaterUnavailableMessage] = useState<
    string | null
  >(null);
  const [releases, setReleases] = useState<AvailableRelease[]>([]);
  const [releasesLoading, setReleasesLoading] = useState(false);
  const [releasesError, setReleasesError] = useState("");
  const [selectedVersion, setSelectedVersion] = useState("");
  const [betaUpdatesEnabled, setBetaUpdatesEnabled] = useState(false);
  const [setupBusy, setSetupBusy] = useState(false);
  const [migrateBusy, setMigrateBusy] = useState(false);
  const [lastMigration, setLastMigration] = useState<MigrateReport | null>(null);

  const applyUpdaterStatus = useCallback(
    (status: { configured: boolean; message: string } | null) => {
      if (!status) {
        setUpdaterUnavailableMessage(
          "Update-Status konnte nicht geladen werden.",
        );
        return;
      }
      setUpdaterUnavailableMessage(
        status.configured ? null : status.message,
      );
    },
    [],
  );

  const loadReleases = useCallback(async () => {
    setReleasesLoading(true);
    setReleasesError("");
    try {
      const [statusInfo, list, betaRaw] = await Promise.all([
        getUpdaterStatus(),
        listAvailableVersions(),
        getSetting("beta_updates_enabled", "false"),
      ]);
      applyUpdaterStatus(statusInfo);
      setBetaUpdatesEnabled(betaRaw.trim().toLowerCase() === "true");
      setReleases(list);
      const firstStable = list.find((r) => !r.prerelease);
      const preferred =
        (appVersion && list.find((r) => r.tag_name === appVersion)?.tag_name) ||
        firstStable?.tag_name ||
        list[0]?.tag_name ||
        "";
      setSelectedVersion(preferred);
    } catch (err) {
      setReleases([]);
      setSelectedVersion("");
      const raw = String(err);
      const looksTechnical =
        /error sending request|dns error|reqwest|os error|failed to lookup|timed out|connection refused/i.test(
          raw,
        );
      setReleasesError(
        looksTechnical
          ? "Versionsliste nicht verfügbar — bitte Internetverbindung prüfen."
          : raw,
      );
      try {
        applyUpdaterStatus(await getUpdaterStatus());
      } catch {
        applyUpdaterStatus(null);
      }
    } finally {
      setReleasesLoading(false);
    }
  }, [appVersion, applyUpdaterStatus]);

  useEffect(() => {
    if (!active) return;
    void loadReleases();
  }, [active, loadReleases]);

  const filteredReleases = useMemo(() => {
    if (betaUpdatesEnabled) return releases;
    return releases.filter((r) => !r.prerelease);
  }, [releases, betaUpdatesEnabled]);

  const installedIsBeta = Boolean(appVersion && isVersionPrerelease(appVersion));

  async function patchBetaUpdates(enabled: boolean) {
    setBetaUpdatesEnabled(enabled);
    try {
      await saveSetting("beta_updates_enabled", enabled ? "true" : "false");
    } catch (err) {
      setBetaUpdatesEnabled(!enabled);
      showError(String(err), "Einstellungen");
    }
  }

  useEffect(() => {
    if (!active || releases.length === 0) return;
    if (appVersion && filteredReleases.some((r) => r.tag_name === appVersion)) {
      setSelectedVersion(appVersion);
      return;
    }
    setSelectedVersion((prev) =>
      filteredReleases.some((r) => r.tag_name === prev)
        ? prev
        : (filteredReleases[0]?.tag_name ?? ""),
    );
  }, [active, appVersion, filteredReleases, releases.length]);

  const selectedRelease = useMemo(
    () => filteredReleases.find((r) => r.tag_name === selectedVersion) ?? null,
    [filteredReleases, selectedVersion],
  );

  const selectedRelation: VersionRelation = useMemo(() => {
    if (!selectedRelease || !appVersion) return null;
    const cmp = compareVersionParts(selectedRelease.tag_name, appVersion);
    if (cmp > 0) return "newer";
    if (cmp < 0) return "older";
    return "same";
  }, [selectedRelease, appVersion]);

  const switchLabel = !selectedRelease?.updater_json_url && selectedRelease?.installer_url
    ? "Installer öffnen…"
    : selectedRelation === "older"
      ? "Ältere Version installieren"
      : selectedRelation === "newer"
        ? "Aktualisieren"
        : "Auf diese Version wechseln";

  const switchDisabled =
    !selectedRelease ||
    selectedRelation === "same" ||
    Boolean(installBlockedReason) ||
    (!selectedRelease.updater_json_url && !selectedRelease.installer_url);

  async function openSetupWizard() {
    setSetupBusy(true);
    try {
      await resetSetup(false);
      onOpenSetupWizard?.();
      onCloseSettings();
    } catch (err) {
      showError(String(err), "Einrichtung");
    } finally {
      setSetupBusy(false);
    }
  }

  async function resetPathsAndOpenWizard() {
    const ok = await confirm(
      "Monitor-, Archiv- und Log-Pfade werden geleert. Cloud-Zugangsdaten und übrige Einstellungen bleiben erhalten. Anschließend startet der Einrichtungsassistent.",
      {
        title: "Pfade zurücksetzen",
        primaryLabel: "Zurücksetzen",
        destructive: true,
      },
    );
    if (!ok) return;
    setSetupBusy(true);
    try {
      await resetSetup(true);
      onOpenSetupWizard?.();
      onCloseSettings();
    } catch (err) {
      showError(String(err), "Einrichtung");
    } finally {
      setSetupBusy(false);
    }
  }

  async function runLegacyMigration() {
    const ok = await confirm(
      "Einstellungen und Zugangsdaten aus der Vorgängerversion werden übernommen, soweit vorhanden. Bereits gesetzte Werte können überschrieben werden.",
      {
        title: "Legacy-Import",
        primaryLabel: "Importieren",
      },
    );
    if (!ok) return;
    setMigrateBusy(true);
    try {
      const report = await migrateLegacySettings(true);
      setLastMigration(report);
      showSuccess(report.message, "Legacy-Import");
    } catch (err) {
      showError(String(err), "Legacy-Import");
    } finally {
      setMigrateBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <p className="text-xs leading-relaxed text-muted">
        Updates, Einrichtung und Import — Aktionen hier greifen sofort, Speichern ist nicht
        nötig.
      </p>

      <SettingsSection
        title="Software-Update"
        description="Aktuelle Version prüfen oder auf eine andere Version wechseln."
      >
        <div className="space-y-4">
          <div className="rounded-md border border-border/60 bg-card/40 px-3 py-2.5">
            <p className="text-xs text-muted">Installierte Version</p>
            <p className="mt-0.5 text-sm font-medium text-foreground">
              {appVersion || "—"}
              {installedIsBeta ? (
                <span className="ml-1.5 text-xs font-medium text-amber-600 dark:text-amber-500">
                  (Beta)
                </span>
              ) : null}
            </p>
            {platformHint ? (
              <p className="mt-2 text-xs text-muted">{platformHint}</p>
            ) : null}
          </div>

          {updaterUnavailableMessage ? (
            <div
              role="status"
              className="rounded-md border border-warning/45 bg-warning/10 px-3 py-2 text-xs text-warning"
            >
              {updaterUnavailableMessage}
            </div>
          ) : null}

          {installBlockedReason ? (
            <div
              role="alert"
              className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              {installBlockedReason}
            </div>
          ) : null}

          <div className="flex flex-wrap items-center gap-3">
            <Button
              type="button"
              variant="secondary"
              disabled={Boolean(installBlockedReason)}
              onClick={() => onRequestUpdateCheck?.(betaUpdatesEnabled)}
            >
              Auf Updates prüfen
            </Button>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={betaUpdatesEnabled}
                onCheckedChange={(v) => void patchBetaUpdates(v === true)}
              />
              Betatester
            </label>
          </div>
          <p className="text-xs text-muted">
            Mit Betatester erhalten Sie Vorabversionen automatisch und sehen sie in der
            Versionsliste.
          </p>

          <div className="space-y-3 border-t border-border/50 pt-3">
            <p className="text-xs font-medium text-foreground">Version wählen</p>

            <div className="space-y-1.5">
              <Label>Ziel-Version</Label>
              <Select
                value={selectedVersion || undefined}
                onValueChange={setSelectedVersion}
                disabled={releasesLoading || filteredReleases.length === 0}
              >
                <SelectTrigger>
                  <SelectValue
                    placeholder={
                      releasesLoading
                        ? "Lade Versionen…"
                        : filteredReleases.length === 0
                          ? "Keine Versionen"
                          : "Version wählen…"
                    }
                  />
                </SelectTrigger>
                <SelectContent>
                  {filteredReleases.map((r) => (
                    <SelectItem key={r.tag_name} value={r.tag_name}>
                      {r.tag_name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {selectedRelease ? (
                <div className="flex flex-wrap gap-1.5 pt-1">
                  {releaseMeta(
                    selectedRelease,
                    filteredReleases.findIndex(
                      (r) => r.tag_name === selectedRelease.tag_name,
                    ),
                    appVersion,
                  ).map((chip) => (
                    <MetaChip key={chip.label} label={chip.label} tone={chip.tone} />
                  ))}
                </div>
              ) : null}
            </div>

            {releasesLoading ? (
              <div className="flex items-center gap-2 text-xs text-muted">
                <Spinner size={14} />
                Versionsliste wird geladen…
              </div>
            ) : null}

            {releasesError ? (
              <div className="space-y-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2">
                <p className="text-xs text-destructive">{releasesError}</p>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => void loadReleases()}
                >
                  Erneut versuchen
                </Button>
              </div>
            ) : null}

            {!releasesLoading && !releasesError && filteredReleases.length === 0 ? (
              <p className="text-xs text-muted">
                Keine Versionen gefunden
                {betaUpdatesEnabled ? "." : " (Vorabversionen sind ausgeblendet)."}
              </p>
            ) : null}

            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="secondary"
                disabled={releasesLoading}
                onClick={() => void loadReleases()}
              >
                <RefreshCw className={cn("h-3.5 w-3.5", releasesLoading && "animate-spin")} />
                Liste neu laden
              </Button>
              <Button
                type="button"
                disabled={switchDisabled}
                onClick={() => {
                  if (!selectedRelease) return;
                  onRequestVersionSwitch?.(selectedRelease);
                }}
              >
                {switchLabel}
              </Button>
            </div>

            {selectedRelease && selectedRelation === "same" ? (
              <p className="text-xs text-muted">Diese Version ist bereits installiert.</p>
            ) : null}

            {selectedRelease && selectedRelation === "older" ? (
              <p className="text-xs text-warning">
                Downgrade: ältere Versionen können Einstellungen oder Funktionen
                zurücksetzen. Installation erst nach Bestätigung im Folgedialog.
              </p>
            ) : null}

            {selectedRelease ? (
              <div className="space-y-2 rounded-md border border-border/50 bg-card/40 p-3">
                <p className="text-sm font-medium text-foreground">
                  Version {selectedRelease.tag_name}
                </p>
                {selectedRelease.published_at ? (
                  <p className="text-xs text-muted">
                    {formatReleaseDate(selectedRelease.published_at)}
                  </p>
                ) : null}
                {!selectedRelease.updater_json_url && selectedRelation !== "same" ? (
                  <p className="text-xs text-muted">
                    Automatische Installation ist für diese Version nicht verfügbar
                    {selectedRelease.installer_url
                      ? " — der Installer kann manuell geöffnet werden."
                      : "."}
                  </p>
                ) : null}
                <ReleaseNotes
                  markdown={selectedRelease.body ?? ""}
                  emptyLabel="Keine Patchnotes verfügbar."
                  className="max-h-40"
                />
              </div>
            ) : null}
          </div>
        </div>
      </SettingsSection>

      <SettingsSection
        title="Einrichtung"
        description="Einrichtungsassistent erneut öffnen oder nur die Kernpfade zurücksetzen."
      >
        <div className="space-y-3">
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="secondary"
              disabled={setupBusy}
              onClick={() => void openSetupWizard()}
            >
              {setupBusy ? <Spinner size={14} /> : null}
              Einrichtungsassistent öffnen
            </Button>
          </div>
          <div className="rounded-md border border-destructive/30 bg-destructive/5 p-3 space-y-2">
            <p className="text-xs font-medium text-foreground">Pfade zurücksetzen</p>
            <p className="text-xs leading-relaxed text-muted">
              Leert Monitor-, Archiv- und Log-Pfad und startet den Assistenten erneut.
              Zugangsdaten und Cloud-Einstellungen bleiben erhalten.
            </p>
            <Button
              type="button"
              variant="destructive"
              disabled={setupBusy}
              onClick={() => void resetPathsAndOpenWizard()}
            >
              Pfade zurücksetzen
            </Button>
          </div>
        </div>
      </SettingsSection>

      <SettingsSection
        title="Legacy-Import"
        description="Einstellungen und Zugangsdaten aus der Vorgängerversion übernehmen."
      >
        <div className="space-y-3">
          <Button
            type="button"
            variant="secondary"
            disabled={migrateBusy}
            onClick={() => void runLegacyMigration()}
          >
            {migrateBusy ? <Spinner size={14} /> : null}
            Import erneut ausführen
          </Button>
          {lastMigration ? (
            <p className="text-xs text-muted">
              Letzter Lauf: {lastMigration.message}
              {lastMigration.skipped
                ? ""
                : ` (${lastMigration.settings_imported} Einstellungen, ${lastMigration.secrets_imported} Secrets)`}
            </p>
          ) : null}
        </div>
      </SettingsSection>
    </div>
  );
}
