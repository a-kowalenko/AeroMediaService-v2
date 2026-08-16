import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Play, Square } from "lucide-react";
import { AppChrome } from "@/components/chrome";
import { AppFeedbackHost } from "@/components/AppFeedbackHost";
import { ConnectionStatusIndicator } from "@/components/ConnectionStatusIndicator";
import { CustomersPanel } from "@/components/CustomersPanel";
import { HistoryTable } from "@/components/HistoryTable";
import { LoadingOverlay } from "@/components/LoadingOverlay";
import { LogConsole } from "@/components/LogConsole";
import { SettingsCluster } from "@/components/SettingsCluster";
import { SettingsDialog } from "@/components/SettingsDialog";
import { SetupWizard } from "@/components/SetupWizard";
import { UpdateDialog } from "@/components/UpdateDialog";
import { UploadPanel } from "@/components/UploadPanel";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  CONNECTION_STATUS_CHANGED,
  MONITORING_STATUS_CHANGED,
  UPDATE_INSTALL_PROGRESS,
  UPLOAD_JOB_ACTIVE,
} from "@/lib/events";
import { showAppToast } from "@/lib/toast";
import {
  autoConnectCloud,
  cancelUpdateInstall,
  checkForUpdates,
  getAppVersion,
  getCloudConnectionStatus,
  getMonitoringStatus,
  getSecret,
  getSetting,
  getUpdaterInstallHint,
  installSpecificVersion,
  installUpdate,
  saveSetting,
  startMonitoring,
  stopMonitoring,
  type AvailableRelease,
  type UpdateInstallProgress,
} from "@/lib/tauri";
import { compareVersionParts } from "@/lib/versionCompare";
import { isCloudConnected, useAppStore } from "@/store/appStore";
import { useLogStore } from "@/store/logStore";
import { initTheme, useThemeStore } from "@/store/themeStore";
import { useUiStore } from "@/store/uiStore";
import "./App.css";

initTheme();

function App() {
  const [version, setVersion] = useState<string>("…");
  const [monitorBusy, setMonitorBusy] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [setupWizardOpen, setSetupWizardOpen] = useState(false);
  const [startupOverlay, setStartupOverlay] = useState(false);
  const [statusLabel, setStatusLabel] = useState("Bereit.");
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [versionInstall, setVersionInstall] = useState<{
    fromVersion: string;
    toVersion: string | null;
    notes: string | null;
    available: boolean;
    message: string;
    updaterJsonUrl: string | null;
    silentAvailable: boolean;
    installerUrl: string | null;
    allowIgnore: boolean;
  } | null>(null);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [updateInstallProgress, setUpdateInstallProgress] =
    useState<UpdateInstallProgress | null>(null);
  const [updaterPlatformHint, setUpdaterPlatformHint] = useState<string | null>(
    null,
  );

  const monitoring = useAppStore((s) => s.monitoring);
  const connectionStatus = useAppStore((s) => s.connectionStatus);
  const uploadJobActive = useAppStore((s) => s.uploadJobActive);
  const setMonitoring = useAppStore((s) => s.setMonitoring);
  const setConnectionStatus = useAppStore((s) => s.setConnectionStatus);
  const setUploadJobActive = useAppStore((s) => s.setUploadJobActive);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const toggleLogOpen = useLogStore((s) => s.toggleOpen);
  const showError = useUiStore((s) => s.showError);
  const showSuccess = useUiStore((s) => s.showSuccess);
  const showWarning = useUiStore((s) => s.showWarning);
  const connected = isCloudConnected(connectionStatus);

  const installBlockedReason = (() => {
    if (updateInstalling) return "Installation läuft bereits…";
    if (uploadJobActive) return "Während dem Upload nicht möglich.";
    return null;
  })();

  async function applyIgnorePreference(
    ignore: boolean,
    latestVersion: string | null | undefined,
  ) {
    if (!latestVersion) return;
    if (ignore) {
      await saveSetting("updater_ignore_version", latestVersion);
      return;
    }
    const current = await getSetting("updater_ignore_version", "");
    if (current === latestVersion) {
      await saveSetting("updater_ignore_version", "");
    }
  }

  async function runUpdateCheck(forceDialog = false) {
    try {
      const result = await checkForUpdates();
      const ignored = (await getSetting("updater_ignore_version", "")).trim();
      const latest = result.latest_version?.trim() ?? "";
      if (
        !forceDialog &&
        result.available &&
        latest &&
        ignored &&
        ignored === latest
      ) {
        return;
      }
      setVersionInstall({
        fromVersion: result.current_version,
        toVersion: result.latest_version,
        notes: result.body,
        available: result.available,
        message: result.message,
        updaterJsonUrl: null,
        silentAvailable: true,
        installerUrl: null,
        allowIgnore: result.available,
      });
      if (forceDialog || result.available) {
        setUpdateDialogOpen(true);
      }
    } catch (e) {
      if (forceDialog) {
        showError(String(e), "Update");
      }
    }
  }

  function openVersionSwitchDialog(release: AvailableRelease) {
    if (installBlockedReason) {
      showError(installBlockedReason, "Update");
      return;
    }
    const from = version === "…" ? "—" : version;
    const cmp = compareVersionParts(release.tag_name, from);
    const isDowngrade = cmp < 0;
    setVersionInstall({
      fromVersion: from,
      toVersion: release.tag_name,
      notes: release.body,
      available: true,
      message: !release.updater_json_url
        ? "Für diese Version ist die automatische Installation nicht verfügbar."
        : isDowngrade
          ? `Zu Version ${release.tag_name} wechseln?`
          : `Update auf ${release.tag_name} verfügbar.`,
      updaterJsonUrl: release.updater_json_url,
      silentAvailable: Boolean(release.updater_json_url),
      installerUrl: release.installer_url,
      allowIgnore: false,
    });
    setUpdateDialogOpen(true);
  }

  async function runInstallVersion() {
    if (!versionInstall || installBlockedReason) return;
    if (!versionInstall.silentAvailable) return;
    setUpdateInstalling(true);
    setUpdateInstallProgress(null);
    try {
      const msg = versionInstall.updaterJsonUrl
        ? await installSpecificVersion(versionInstall.updaterJsonUrl)
        : await installUpdate();
      showSuccess(msg, "Update");
      try {
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      } catch {
        showWarning(
          "Version installiert — bitte App manuell neu starten.",
          "Update",
        );
      }
    } catch (e) {
      const msg = String(e);
      if (!/abgebrochen/i.test(msg)) {
        showError(msg, "Update");
      }
    } finally {
      setUpdateInstalling(false);
      setUpdateInstallProgress(null);
    }
  }

  async function cancelInstallVersion() {
    if (!updateInstalling) return;
    if (updateInstallProgress?.phase === "install") return;
    try {
      await cancelUpdateInstall();
    } catch {
      // ignore
    }
  }

  useEffect(() => {
    getAppVersion()
      .then(setVersion)
      .catch(() => setVersion("unknown"));
    getMonitoringStatus()
      .then(setMonitoring)
      .catch(() => {});
    getCloudConnectionStatus()
      .then(setConnectionStatus)
      .catch(() => {});
    getUpdaterInstallHint()
      .then(setUpdaterPlatformHint)
      .catch(() => setUpdaterPlatformHint(null));
    void (async () => {
      try {
        const theme = (await getSetting("ui_theme", "dark")).trim().toLowerCase();
        if (theme === "light" || theme === "dark") {
          setThemeMode(theme);
        }
        const setup = (await getSetting("setup_completed", "false"))
          .trim()
          .toLowerCase();
        if (setup !== "true") {
          setSetupWizardOpen(true);
        }
      } catch {
        setSetupWizardOpen(true);
      }
    })();
  }, [setMonitoring, setConnectionStatus, setThemeMode]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    listen<boolean>(MONITORING_STATUS_CHANGED, (event) => {
      setMonitoring(Boolean(event.payload));
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});
    listen<string>(CONNECTION_STATUS_CHANGED, (event) => {
      setConnectionStatus(String(event.payload ?? ""));
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});
    listen<boolean>(UPLOAD_JOB_ACTIVE, (event) => {
      setUploadJobActive(Boolean(event.payload));
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});
    listen<UpdateInstallProgress>(UPDATE_INSTALL_PROGRESS, (event) => {
      setUpdateInstallProgress(event.payload);
    })
      .then((fn) => unlisteners.push(fn))
      .catch(() => {});
    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [setMonitoring, setConnectionStatus, setUploadJobActive]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "j") {
        e.preventDefault();
        toggleLogOpen();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggleLogOpen]);

  // Deferred startup: auto-connect after first paint (legacy main.py + deferred_startup).
  useEffect(() => {
    if (setupWizardOpen) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const selected = (
            await getSetting("selected_cloud_service", "dropbox")
          )
            .trim()
            .toLowerCase();
          let shouldConnect = false;
          if (selected === "custom_api") {
            const [url, token] = await Promise.all([
              getSecret("custom_api_url"),
              getSecret("custom_api_bearer_token"),
            ]);
            shouldConnect = Boolean(url?.trim() && token?.trim());
          } else {
            const refresh = await getSecret("db_refresh_token");
            shouldConnect = Boolean(refresh?.trim());
          }

          if (!shouldConnect) {
            if (!cancelled) setStatusLabel("Bereit.");
          } else {
            setStatusLabel("Verbindung wird hergestellt…");
            setStartupOverlay(true);
            const result = await autoConnectCloud();
            if (cancelled) return;
            if (result.success) {
              setConnectionStatus(result.status || "Verbunden");
              try {
                await startMonitoring();
              } catch (err) {
                showError(String(err), "Monitoring");
              }
              setStatusLabel(result.message || "Bereit.");
              showAppToast(result.message || "Verbunden.", {
                tone: "success",
                title: "Cloud",
              });
            } else {
              setStatusLabel(result.message || "Bereit.");
              if (result.status && result.status !== "Nicht verbunden") {
                showError(result.message, "Auto-Connect");
              }
            }
          }
        } catch (err) {
          if (!cancelled) {
            setStatusLabel("Bereit.");
            showError(String(err), "Auto-Connect");
          }
        } finally {
          if (!cancelled) {
            setStartupOverlay(false);
            void runUpdateCheck(false);
          }
        }
      })();
    }, 0);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [setConnectionStatus, setupWizardOpen, showError]);

  async function onStart() {
    setMonitorBusy(true);
    try {
      await startMonitoring();
      setStatusLabel("Monitoring aktiv.");
      showAppToast("Monitoring gestartet.", { tone: "success" });
    } catch (err) {
      showError(String(err), "Monitoring");
    } finally {
      setMonitorBusy(false);
    }
  }

  async function onStop() {
    setMonitorBusy(true);
    try {
      await stopMonitoring();
      setStatusLabel("Monitoring gestoppt.");
      showAppToast("Monitoring gestoppt.", { tone: "info" });
    } catch (err) {
      showError(String(err), "Monitoring");
    } finally {
      setMonitorBusy(false);
    }
  }

  return (
    <div className="app-root">
      <LoadingOverlay
        visible={startupOverlay}
        message="Verbindung wird hergestellt…"
      />

      <AppChrome
        actions={
          <>
            <ConnectionStatusIndicator />
            <Button
              type="button"
              size="sm"
              disabled={monitorBusy || monitoring}
              onClick={() => void onStart()}
              title="Monitoring starten"
            >
              <Play className="h-3.5 w-3.5" />
              <span className="hidden sm:inline">Start</span>
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className={
                monitoring
                  ? "border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
                  : undefined
              }
              disabled={monitorBusy || !monitoring}
              onClick={() => void onStop()}
              title="Monitoring stoppen"
            >
              <Square className="h-3.5 w-3.5" />
              <span className="hidden sm:inline">Stop</span>
            </Button>
            <SettingsCluster onOpenSettings={() => setSettingsOpen(true)} />
          </>
        }
      >
        <div className="pointer-events-none flex min-w-0 items-center gap-2.5">
          <div className="flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-lg bg-primary-soft ring-1 ring-primary/20">
            <img
              src="/logo.png"
              alt=""
              className="h-[22px] w-[22px] object-contain"
              onError={(e) => {
                (e.target as HTMLImageElement).style.display = "none";
              }}
            />
          </div>
          <div className="flex min-h-[34px] min-w-0 flex-col justify-center gap-0.5">
            <div className="flex min-w-0 items-baseline gap-x-1.5">
              <h1 className="font-display truncate text-base font-semibold leading-none tracking-tight text-primary">
                Aero Media Service
              </h1>
              <span className="shrink-0 text-[11px] leading-none text-muted">
                v{version}
              </span>
            </div>
            <p className="truncate text-[10px] leading-none text-muted">
              {monitoring
                ? "Monitoring aktiv"
                : connected
                  ? "Bereit"
                  : "Nicht verbunden"}
            </p>
          </div>
        </div>
      </AppChrome>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex min-h-0 flex-1">
          <aside className="ams-sidebar-bg flex w-full max-w-md flex-col border-r border-border backdrop-blur-md sm:w-[380px]">
            <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3.5 [scrollbar-gutter:stable]">
              <UploadPanel compact />
            </div>

            <div className="border-t border-border bg-gradient-to-t from-card/90 to-card/40 px-3.5 py-2.5 backdrop-blur-sm">
              <p className="truncate text-xs text-muted" title={statusLabel}>
                {statusLabel}
                {connectionStatus ? ` · ${connectionStatus}` : ""}
              </p>
            </div>
          </aside>

          <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
            <Tabs
              defaultValue="history"
              className="flex min-h-0 flex-1 flex-col overflow-hidden"
            >
              <div className="shrink-0 border-b border-border px-3 pt-2 sm:px-4">
                <TabsList>
                  <TabsTrigger value="history">Historie</TabsTrigger>
                  <TabsTrigger value="customers">Kunden</TabsTrigger>
                </TabsList>
              </div>
              <TabsContent
                value="history"
                className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden"
              >
                <HistoryTable />
              </TabsContent>
              <TabsContent
                value="customers"
                className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden"
              >
                <CustomersPanel />
              </TabsContent>
            </Tabs>
          </main>
        </div>

        <LogConsole />
      </div>

      <SettingsDialog
        open={settingsOpen && !(updateDialogOpen && updateInstalling)}
        onClose={() => {
          if (updateDialogOpen || updateInstalling) return;
          setSettingsOpen(false);
        }}
        appVersion={version === "…" ? "" : version}
        platformHint={updaterPlatformHint}
        installBlockedReason={installBlockedReason}
        onRequestUpdateCheck={() => void runUpdateCheck(true)}
        onRequestVersionSwitch={openVersionSwitchDialog}
        onOpenSetupWizard={() => {
          setSettingsOpen(false);
          setSetupWizardOpen(true);
        }}
      />

      <SetupWizard
        open={setupWizardOpen}
        onComplete={() => {
          setSetupWizardOpen(false);
          setStatusLabel("Einrichtung abgeschlossen.");
        }}
      />

      <UpdateDialog
        open={updateDialogOpen}
        fromVersion={versionInstall?.fromVersion ?? version}
        toVersion={versionInstall?.toVersion ?? null}
        notes={versionInstall?.notes ?? null}
        available={Boolean(versionInstall?.available)}
        message={versionInstall?.message ?? ""}
        installing={updateInstalling}
        installProgress={updateInstallProgress}
        silentAvailable={versionInstall?.silentAvailable ?? true}
        blockedReason={installBlockedReason}
        platformHint={updaterPlatformHint}
        installerUrl={versionInstall?.installerUrl ?? null}
        allowIgnore={versionInstall?.allowIgnore ?? false}
        onInstall={() => void runInstallVersion()}
        onCancelInstall={() => void cancelInstallVersion()}
        onLater={(ignore) => {
          if (updateInstalling) return;
          void applyIgnorePreference(ignore, versionInstall?.toVersion);
          setUpdateDialogOpen(false);
        }}
        onClose={(ignore) => {
          if (updateInstalling) return;
          void applyIgnorePreference(ignore, versionInstall?.toVersion);
          setUpdateDialogOpen(false);
        }}
      />

      <AppFeedbackHost />
    </div>
  );
}

export default App;
