import {FormEvent, type ReactNode, useCallback, useEffect, useMemo, useState} from "react";
import {open as openDirectoryDialog} from "@tauri-apps/plugin-dialog";
import {openUrl} from "@tauri-apps/plugin-opener";
import {FolderOpen, Moon, Sun} from "lucide-react";
import {Spinner} from "@/components/Spinner";
import {StatusChip} from "@/components/StatusChip";
import {SettingsSection} from "@/components/settings/SettingsSection";
import {Button} from "@/components/ui/button";
import {Checkbox} from "@/components/ui/checkbox";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import {Input} from "@/components/ui/input";
import {Label} from "@/components/ui/label";
import {PasswordInput} from "@/components/ui/password-input";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import {Tabs, TabsContent, TabsList, TabsTrigger} from "@/components/ui/tabs";
import {
    applyBridgeConfig,
    connectCustomApi,
    connectDropbox,
    disconnectCustomApi,
    disconnectDropbox,
    finishDropboxOauth,
    getAtsHostDetails,
    getAtsHostsSummary,
    getBridgeStatus,
    getSecret,
    getSetting,
    getSmsBalance,
    getUpdaterStatus,
    listAvailableVersions,
    migrateLegacySettings,
    resetSetup,
    saveSecret,
    saveSetting,
    testLinkShortener,
    verifyDropboxStatus,
    type AtsHostDetails,
    type AtsHostSummary,
    type AvailableRelease,
} from "@/lib/tauri";
import {AtsActivityEventCard} from "@/components/AtsActivityEventCard";
import {
    atsPresenceChipLabel,
    atsPresenceChipTone,
    defaultAtsHostSelection,
    findAtsHost,
} from "@/lib/atsPresence";
import {
    AtsHostListSections,
    countActiveAtsHosts,
    countConnectedAtsHosts,
} from "@/components/AtsHostListSections";
import {showAppToast} from "@/lib/toast";
import {eventTypeLabel} from "@/lib/atsActivityDisplay";
import {compareVersionParts} from "@/lib/versionCompare";
import {useThemeStore, type ThemeMode} from "@/store/themeStore";
import {useUiStore} from "@/store/uiStore";

type TabId =
    | "general"
    | "cloud"
    | "email"
    | "sms"
    | "shortener"
    | "whatsapp"
    | "extras";

type Props = {
    open: boolean;
    onClose: () => void;
    appVersion?: string;
    platformHint?: string | null;
    installBlockedReason?: string | null;
    onRequestUpdateCheck?: () => void;
    onRequestVersionSwitch?: (release: AvailableRelease) => void;
    onOpenSetupWizard?: () => void;
};

const TAB_ITEMS: { id: TabId; label: string }[] = [
    {id: "general", label: "Allgemein"},
    {id: "cloud", label: "Cloud-Dienst"},
    {id: "email", label: "E-Mail"},
    {id: "sms", label: "SMS"},
    {id: "shortener", label: "Link-Shortener"},
    {id: "whatsapp", label: "WhatsApp"},
    {id: "extras", label: "Extras"},
];

const SHORTENER_PRESETS = [
    {value: "permanent", label: "Permanent"},
    {value: "14d", label: "14 Tage"},
    {value: "1m", label: "1 Monat"},
    {value: "3m", label: "3 Monate"},
    {value: "6m", label: "6 Monate"},
    {value: "1y", label: "1 Jahr"},
] as const;

/** Canonical Custom-API upload modes (legacy-compatible). */
const CUSTOM_API_UPLOAD_MODE_PROXIED = "proxied_session";
const CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX = "direct_dropbox_complete";

function normalizeCustomApiUploadMode(raw: string | null | undefined): string {
    const v = (raw ?? "").trim();
    if (v === CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX || v === "direct") {
        return CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX;
    }
    return CUSTOM_API_UPLOAD_MODE_PROXIED;
}

type SettingsFormSnapshot = {
    themeMode: ThemeMode;
    general: {
        monitor_path: string;
        archive_path: string;
        log_file_path: string;
        scan_interval: string;
        folder_stability_enabled: boolean;
        folder_stability_seconds: string;
        bridge_enabled: boolean;
        bridge_bind: string;
        bridge_token: string;
    };
    cloudService: "dropbox" | "custom_api";
    dropbox: { db_app_key: string; db_app_secret: string };
    customApi: {
        custom_api_url: string;
        custom_api_bearer_token: string;
        aero_customer_base_url: string;
        aero_customer_api_token: string;
        custom_api_upload_endpoint: string;
        custom_api_share_endpoint: string;
        custom_api_health_endpoint: string;
        custom_api_upload_mode: string;
        custom_db_app_key: string;
        custom_db_app_secret: string;
    };
    email: {
        smtp_host: string;
        smtp_port: string;
        smtp_user: string;
        smtp_pass: string;
        smtp_sender_addr: string;
        smtp_sender_name: string;
        smtp_fallback_recipient: string;
        smtp_sandbox_mode: boolean;
        imap_save_sent_enabled: boolean;
        imap_host: string;
        imap_port: string;
        imap_sent_folder: string;
        imap_same_credentials: boolean;
        imap_user: string;
        imap_pass: string;
    };
    sms: {
        seven_api_key: string;
        seven_sandbox_api_key: string;
        seven_sender: string;
        seven_sandbox_mode: boolean;
    };
    shortener: {
        link_shortener_enabled: boolean;
        shortener_base_url: string;
        shortener_api_key: string;
        shortener_expires_preset: string;
    };
    whatsapp: {
        twilio_account_sid: string;
        twilio_auth_token: string;
        twilio_whatsapp_from: string;
    };
};

function captureSettingsFormSnapshot(input: SettingsFormSnapshot): SettingsFormSnapshot {
    return JSON.parse(JSON.stringify(input)) as SettingsFormSnapshot;
}

function settingsSnapshotsEqual(
    a: SettingsFormSnapshot | null,
    b: SettingsFormSnapshot | null,
): boolean {
    if (!a || !b) return false;
    return JSON.stringify(a) === JSON.stringify(b);
}

function monitorSettingsChanged(
    snapshot: SettingsFormSnapshot | null,
    general: SettingsFormSnapshot["general"],
    scan: string,
    stability: string,
): boolean {
    if (!snapshot) return false;
    const prev = snapshot.general;
    return (
        prev.monitor_path.trim() !== general.monitor_path.trim() ||
        prev.scan_interval !== scan ||
        prev.folder_stability_enabled !== general.folder_stability_enabled ||
        prev.folder_stability_seconds !== stability
    );
}

function Field({label, children}: { label: string; children: ReactNode }) {
    return (
        <div className="space-y-1.5">
            <Label>{label}</Label>
            {children}
        </div>
    );
}

function InlineStatus({
                          label,
                          value,
                          loading = false,
                      }: {
    label: string;
    value: string;
    loading?: boolean;
}) {
    return (
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted">
      <span>
        {label}: {value}
      </span>
            {loading ? (
                <span className="inline-flex items-center gap-1.5">
          <Spinner size={12} className="border-[1.5px]"/>
          <span>wird geladen…</span>
        </span>
            ) : null}
        </div>
    );
}

function AtsPresenceChip({
                             label,
                             tone,
                         }: {
    label: string;
    tone: "active" | "inactive" | "degraded" | "neutral";
}) {
    const toneClass =
        tone === "active"
            ? "border-success/40 bg-success/10 text-success"
            : tone === "inactive"
                ? "border-border/70 bg-muted/30 text-muted"
                : tone === "degraded"
                    ? "border-warning/45 bg-warning/10 text-warning"
                    : "border-primary/40 bg-primary/10 text-primary";
    return (
        <span
            className={`inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none ${toneClass}`}
        >
      {label}
    </span>
    );
}

function formatTimestamp(value: string): string {
    if (!value.trim()) return "—";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return new Intl.DateTimeFormat("de-DE", {
        dateStyle: "short",
        timeStyle: "short",
    }).format(date);
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
    placeholder?: string;
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
                <Button
                    type="button"
                    variant="secondary"
                    size="icon"
                    onClick={onPick}
                    title="Ordner wählen"
                >
                    <FolderOpen className="h-4 w-4"/>
                </Button>
            </div>
        </div>
    );
}

function boolFromSetting(value: string, fallback = false): boolean {
    const v = value.trim().toLowerCase();
    if (!v) return fallback;
    return v !== "false" && v !== "0" && v !== "no";
}

/** Persist a secret only when non-empty — empty bulk-save must not wipe the keyring. */
async function persistSecret(key: string, value: string): Promise<void> {
    const trimmed = value.trim();
    if (!trimmed) return;
    await saveSecret(key, trimmed);
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

async function promptAuthCode(authorizeUrl: string): Promise<string | null> {
    const ui = useUiStore.getState();
    const proceed = await ui.confirm(
        "Ein Browser-Fenster wird geöffnet, um die App zu autorisieren.\n\n" +
        "Bitte kopieren Sie den angezeigten Code und fügen Sie ihn im nächsten Dialog ein.",
        {
            title: "Dropbox autorisieren",
            primaryLabel: "Browser öffnen",
            secondaryLabel: "Abbrechen",
        },
    );
    if (!proceed) return null;
    try {
        await openUrl(authorizeUrl);
    } catch {
        window.open(authorizeUrl, "_blank", "noopener,noreferrer");
    }
    const code = await ui.prompt("Eingabe-Code von Dropbox:", {
        title: "Autorisierungscode",
        placeholder: "Code einfügen…",
        primaryLabel: "Weiter",
    });
    return code?.trim() || null;
}

export function SettingsDialog({
                                   open: isOpen,
                                   onClose,
                                   appVersion = "",
                                   platformHint = null,
                                   installBlockedReason = null,
                                   onRequestUpdateCheck,
                                   onRequestVersionSwitch,
                                   onOpenSetupWizard,
                               }: Props) {
    const themeMode = useThemeStore((s) => s.mode);
    const setThemeMode = useThemeStore((s) => s.setMode);
    const showError = useUiStore((s) => s.showError);
    const showSuccess = useUiStore((s) => s.showSuccess);
    const confirm = useUiStore((s) => s.confirm);
    const [tab, setTab] = useState<TabId>("general");
    const [busy, setBusy] = useState(false);
    const [loadError, setLoadError] = useState("");
    const [savedSnapshot, setSavedSnapshot] = useState<SettingsFormSnapshot | null>(null);

    const [updaterMessage, setUpdaterMessage] = useState("—");
    const [releases, setReleases] = useState<AvailableRelease[]>([]);
    const [releasesLoading, setReleasesLoading] = useState(false);
    const [releasesError, setReleasesError] = useState("");
    const [selectedVersion, setSelectedVersion] = useState("");
    const [showPrereleases, setShowPrereleases] = useState(false);
    const [general, setGeneral] = useState({
        monitor_path: "",
        archive_path: "",
        log_file_path: "",
        scan_interval: "10",
        folder_stability_enabled: true,
        folder_stability_seconds: "15",
        bridge_enabled: false,
        bridge_bind: "0.0.0.0:8787",
        bridge_token: "",
    });
    const [bridgeStatusLabel, setBridgeStatusLabel] = useState("—");
    const [bridgeBusy, setBridgeBusy] = useState(false);
    const [bridgeStatusLoading, setBridgeStatusLoading] = useState(false);
    const [bridgeView, setBridgeView] = useState<"monitoring" | "clients">("monitoring");
    const [atsHosts, setAtsHosts] = useState<AtsHostSummary[]>([]);
    const [atsHostsLoading, setAtsHostsLoading] = useState(false);
    const [atsHostsError, setAtsHostsError] = useState("");
    const [selectedAtsHostId, setSelectedAtsHostId] = useState("");
    const [selectedAtsDetails, setSelectedAtsDetails] = useState<AtsHostDetails | null>(null);
    const [atsDetailsLoading, setAtsDetailsLoading] = useState(false);

    const [cloudService, setCloudService] = useState<"dropbox" | "custom_api">("dropbox");
    const [dropbox, setDropbox] = useState({
        db_app_key: "",
        db_app_secret: "",
    });
    const [dbStatus, setDbStatus] = useState("Nicht verbunden");
    const [dbBusy, setDbBusy] = useState(false);
    const [dbStatusLoading, setDbStatusLoading] = useState(false);

    const [customApi, setCustomApi] = useState({
        custom_api_url: "",
        custom_api_bearer_token: "",
        aero_customer_base_url: "",
        aero_customer_api_token: "",
        custom_api_upload_endpoint: "/upload",
        custom_api_share_endpoint: "/share",
        custom_api_health_endpoint: "/health",
        custom_api_upload_mode: "proxied_session",
        custom_db_app_key: "",
        custom_db_app_secret: "",
    });
    const [customApiStatus, setCustomApiStatus] = useState("Nicht verbunden");
    const [customDbStatus, setCustomDbStatus] = useState("Nicht verbunden");
    const [customBusy, setCustomBusy] = useState(false);
    const [customDbStatusLoading, setCustomDbStatusLoading] = useState(false);

    const [email, setEmail] = useState({
        smtp_host: "",
        smtp_port: "587",
        smtp_user: "",
        smtp_pass: "",
        smtp_sender_addr: "",
        smtp_sender_name: "Dropbox Uploader",
        smtp_fallback_recipient: "",
        smtp_sandbox_mode: false,
        imap_save_sent_enabled: true,
        imap_host: "",
        imap_port: "993",
        imap_sent_folder: "",
        imap_same_credentials: true,
        imap_user: "",
        imap_pass: "",
    });

    const [sms, setSms] = useState({
        seven_api_key: "",
        seven_sandbox_api_key: "",
        seven_sender: "",
        seven_sandbox_mode: false,
    });
    const [smsBalance, setSmsBalance] = useState("Unbekannt");
    const [smsBusy, setSmsBusy] = useState(false);
    const [smsBalanceLoading, setSmsBalanceLoading] = useState(false);

    const [shortener, setShortener] = useState({
        link_shortener_enabled: false,
        shortener_base_url: "",
        shortener_api_key: "",
        shortener_expires_preset: "permanent",
    });
    const [shortenerBusy, setShortenerBusy] = useState(false);

    const [whatsapp, setWhatsapp] = useState({
        twilio_account_sid: "",
        twilio_auth_token: "",
        twilio_whatsapp_from: "",
    });

    const currentSnapshot = useMemo(
        (): SettingsFormSnapshot =>
            captureSettingsFormSnapshot({
                themeMode,
                general,
                cloudService,
                dropbox,
                customApi,
                email,
                sms,
                shortener,
                whatsapp,
            }),
        [themeMode, general, cloudService, dropbox, customApi, email, sms, shortener, whatsapp],
    );

    const isDirty =
        savedSnapshot !== null && !settingsSnapshotsEqual(currentSnapshot, savedSnapshot);

    const requestClose = useCallback(async () => {
        if (busy) return;
        if (isDirty) {
            const discard = await confirm(
                "Ungespeicherte Änderungen verwerfen?",
                {
                    title: "Einstellungen",
                    primaryLabel: "Verwerfen",
                    secondaryLabel: "Abbrechen",
                    destructive: true,
                },
            );
            if (!discard) return;
        }
        onClose();
    }, [busy, isDirty, confirm, onClose]);

    const filteredReleases = useMemo(() => {
        if (showPrereleases) return releases;
        return releases.filter((r) => !r.prerelease);
    }, [releases, showPrereleases]);

    const selectedRelease = useMemo(
        () => filteredReleases.find((r) => r.tag_name === selectedVersion) ?? null,
        [filteredReleases, selectedVersion],
    );

    const selectedRelation = useMemo(() => {
        if (!selectedRelease || !appVersion) return null;
        const cmp = compareVersionParts(selectedRelease.tag_name, appVersion);
        if (cmp > 0) return "newer" as const;
        if (cmp < 0) return "older" as const;
        return "same" as const;
    }, [selectedRelease, appVersion]);

    const connectedAtsHostsCount = useMemo(
        () => countConnectedAtsHosts(atsHosts),
        [atsHosts],
    );
    const selectedAtsHost = useMemo(
        () => findAtsHost(atsHosts, selectedAtsHostId),
        [atsHosts, selectedAtsHostId],
    );
    const activeAtsHostsCount = useMemo(
        () => countActiveAtsHosts(atsHosts),
        [atsHosts],
    );

    const loadExtras = useCallback(async () => {
        setReleasesLoading(true);
        setReleasesError("");
        try {
            const [statusInfo, list] = await Promise.all([
                getUpdaterStatus(),
                listAvailableVersions(),
            ]);
            setUpdaterMessage(statusInfo.message);
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
                const statusInfo = await getUpdaterStatus();
                setUpdaterMessage(statusInfo.message);
            } catch {
                setUpdaterMessage("Update-Status nicht verfügbar.");
            }
        } finally {
            setReleasesLoading(false);
        }
    }, [appVersion]);

    const loadAtsHosts = useCallback(async () => {
        setAtsHostsLoading(true);
        setAtsHostsError("");
        try {
            const items = await getAtsHostsSummary(60);
            setAtsHosts(items);
            setSelectedAtsHostId((prev) => {
                if (prev && items.some((host) => host.instance_id === prev)) return prev;
                return defaultAtsHostSelection(items);
            });
        } catch (err) {
            setAtsHosts([]);
            setSelectedAtsHostId("");
            setSelectedAtsDetails(null);
            setAtsHostsError(String(err));
        } finally {
            setAtsHostsLoading(false);
        }
    }, []);

    const loadAtsDetails = useCallback(async (instanceId: string) => {
        const id = instanceId.trim();
        if (!id) {
            setSelectedAtsDetails(null);
            return;
        }
        setAtsDetailsLoading(true);
        try {
            const details = await getAtsHostDetails(id, 60, 100);
            setSelectedAtsDetails(details);
        } catch (err) {
            setSelectedAtsDetails(null);
            setAtsHostsError(String(err));
        } finally {
            setAtsDetailsLoading(false);
        }
    }, []);

    useEffect(() => {
        if (!isOpen || tab !== "extras") return;
        void loadExtras();
    }, [isOpen, tab, loadExtras]);

    useEffect(() => {
        if (!isOpen || tab !== "extras" || releases.length === 0) return;
        const visible = showPrereleases
            ? releases
            : releases.filter((r) => !r.prerelease);
        if (appVersion && visible.some((r) => r.tag_name === appVersion)) {
            setSelectedVersion(appVersion);
            return;
        }
        setSelectedVersion((prev) =>
            visible.some((r) => r.tag_name === prev)
                ? prev
                : (visible[0]?.tag_name ?? ""),
        );
    }, [appVersion, isOpen, releases, showPrereleases, tab]);

    const loadAll = useCallback(async () => {
        setLoadError("");
        setSavedSnapshot(null);
        try {
            const [
                monitor_path,
                archive_path,
                log_file_path,
                scan_interval,
                stability_enabled,
                folder_stability_seconds,
                bridge_enabled,
                bridge_bind,
                selected_cloud_service,
                custom_api_upload_endpoint,
                custom_api_share_endpoint,
                custom_api_health_endpoint,
                custom_api_upload_mode,
                smtp_host,
                smtp_port,
                smtp_sender_addr,
                smtp_sender_name,
                smtp_fallback_recipient,
                smtp_sandbox_mode,
                imap_save_sent_enabled,
                imap_host,
                imap_port,
                imap_sent_folder,
                imap_same_credentials,
                seven_sender,
                seven_sandbox_mode,
                link_shortener_enabled,
                shortener_expires_preset,
                twilio_whatsapp_from,
            ] = await Promise.all([
                getSetting("monitor_path", ""),
                getSetting("archive_path", ""),
                getSetting("log_file_path", ""),
                getSetting("scan_interval", "10"),
                getSetting("folder_stability_enabled", "true"),
                getSetting("folder_stability_seconds", "15"),
                getSetting("bridge_enabled", "false"),
                getSetting("bridge_bind", "0.0.0.0:8787"),
                getSetting("selected_cloud_service", "dropbox"),
                getSetting("custom_api_upload_endpoint", "/upload"),
                getSetting("custom_api_share_endpoint", "/share"),
                getSetting("custom_api_health_endpoint", "/health"),
                getSetting("custom_api_upload_mode", "proxied_session"),
                getSetting("smtp_host", ""),
                getSetting("smtp_port", "587"),
                getSetting("smtp_sender_addr", ""),
                getSetting("smtp_sender_name", "Dropbox Uploader"),
                getSetting("smtp_fallback_recipient", ""),
                getSetting("smtp_sandbox_mode", "false"),
                getSetting("imap_save_sent_enabled", "true"),
                getSetting("imap_host", ""),
                getSetting("imap_port", "993"),
                getSetting("imap_sent_folder", ""),
                getSetting("imap_same_credentials", "true"),
                getSetting("seven_sender", ""),
                getSetting("seven_sandbox_mode", "false"),
                getSetting("link_shortener_enabled", "false"),
                getSetting("shortener_expires_preset", "permanent"),
                getSetting("twilio_whatsapp_from", ""),
            ]);

            setGeneral({
                monitor_path,
                archive_path,
                log_file_path,
                scan_interval,
                folder_stability_enabled: boolFromSetting(stability_enabled, true),
                folder_stability_seconds,
                bridge_enabled: boolFromSetting(bridge_enabled),
                bridge_bind: bridge_bind || "0.0.0.0:8787",
                bridge_token: "",
            });
            setBridgeStatusLoading(true);
            void getBridgeStatus()
                .then((status) => {
                    setBridgeStatusLabel(
                        status.running
                            ? `Aktiv auf ${status.bind_addr}`
                            : status.last_error
                                ? `Inaktiv (${status.last_error})`
                                : "Inaktiv",
                    );
                })
                .catch(() => {
                    setBridgeStatusLabel("—");
                })
                .finally(() => {
                    setBridgeStatusLoading(false);
                });
            setCloudService(selected_cloud_service === "custom_api" ? "custom_api" : "dropbox");
            setEmail((prev) => ({
                ...prev,
                smtp_host,
                smtp_port,
                smtp_sender_addr,
                smtp_sender_name,
                smtp_fallback_recipient,
                smtp_sandbox_mode: boolFromSetting(smtp_sandbox_mode),
                imap_save_sent_enabled: boolFromSetting(imap_save_sent_enabled, true),
                imap_host,
                imap_port,
                imap_sent_folder,
                imap_same_credentials: boolFromSetting(imap_same_credentials, true),
            }));
            setSms((prev) => ({
                ...prev,
                seven_sender,
                seven_sandbox_mode: boolFromSetting(seven_sandbox_mode),
            }));
            setShortener((prev) => ({
                ...prev,
                link_shortener_enabled: boolFromSetting(link_shortener_enabled),
                shortener_expires_preset: shortener_expires_preset || "permanent",
            }));
            setWhatsapp((prev) => ({...prev, twilio_whatsapp_from}));
            setCustomApi((prev) => ({
                ...prev,
                custom_api_upload_endpoint: custom_api_upload_endpoint || "/upload",
                custom_api_share_endpoint: custom_api_share_endpoint || "/share",
                custom_api_health_endpoint: custom_api_health_endpoint || "/health",
                custom_api_upload_mode: normalizeCustomApiUploadMode(custom_api_upload_mode),
            }));

            try {
                const [
                    db_app_key,
                    db_app_secret,
                    custom_api_url,
                    custom_api_bearer_token,
                    aero_customer_base_url,
                    aero_customer_api_token,
                    custom_db_app_key,
                    custom_db_app_secret,
                    smtp_user,
                    smtp_pass,
                    imap_user,
                    imap_pass,
                    seven_api_key,
                    seven_sandbox_api_key,
                    shortener_base_url,
                    shortener_api_key,
                    skylink_api_url,
                    skylink_api_key,
                    twilio_account_sid,
                    twilio_auth_token,
                    bridge_token,
                ] = await Promise.all([
                    getSecret("db_app_key"),
                    getSecret("db_app_secret"),
                    getSecret("custom_api_url"),
                    getSecret("custom_api_bearer_token"),
                    getSecret("aero_customer_base_url"),
                    getSecret("aero_customer_api_token"),
                    getSecret("custom_db_app_key"),
                    getSecret("custom_db_app_secret"),
                    getSecret("smtp_user"),
                    getSecret("smtp_pass"),
                    getSecret("imap_user"),
                    getSecret("imap_pass"),
                    getSecret("seven_api_key"),
                    getSecret("seven_sandbox_api_key"),
                    getSecret("shortener_base_url"),
                    getSecret("shortener_api_key"),
                    getSecret("skylink_api_url"),
                    getSecret("skylink_api_key"),
                    getSecret("twilio_account_sid"),
                    getSecret("twilio_auth_token"),
                    getSecret("bridge_token"),
                ]);

                setDropbox({
                    db_app_key: db_app_key ?? "",
                    db_app_secret: db_app_secret ?? "",
                });
                setGeneral((prev) => ({
                    ...prev,
                    bridge_token: bridge_token ?? "",
                }));
                setCustomApi((prev) => ({
                    ...prev,
                    custom_api_url: custom_api_url ?? "",
                    custom_api_bearer_token: custom_api_bearer_token ?? "",
                    aero_customer_base_url: aero_customer_base_url ?? "",
                    aero_customer_api_token: aero_customer_api_token ?? "",
                    custom_db_app_key: custom_db_app_key ?? "",
                    custom_db_app_secret: custom_db_app_secret ?? "",
                }));
                setEmail((prev) => ({
                    ...prev,
                    smtp_user: smtp_user ?? "",
                    smtp_pass: smtp_pass ?? "",
                    imap_user: imap_user ?? "",
                    imap_pass: imap_pass ?? "",
                }));
                setSms((prev) => ({
                    ...prev,
                    seven_api_key: seven_api_key ?? "",
                    seven_sandbox_api_key: seven_sandbox_api_key ?? "",
                }));
                let base = shortener_base_url ?? "";
                if (!base && skylink_api_url) {
                    base = skylink_api_url
                        .replace(/\/api\/shorten\/?$/i, "")
                        .replace(/\/api\/create\/?$/i, "");
                }
                setShortener((prev) => ({
                    ...prev,
                    shortener_base_url: base,
                    shortener_api_key: shortener_api_key || skylink_api_key || "",
                }));
                setWhatsapp((prev) => ({
                    ...prev,
                    twilio_account_sid: twilio_account_sid ?? "",
                    twilio_auth_token: twilio_auth_token ?? "",
                }));

                setSavedSnapshot(
                    captureSettingsFormSnapshot({
                        themeMode: useThemeStore.getState().mode,
                        general: {
                            monitor_path,
                            archive_path,
                            log_file_path,
                            scan_interval,
                            folder_stability_enabled: boolFromSetting(stability_enabled, true),
                            folder_stability_seconds,
                            bridge_enabled: boolFromSetting(bridge_enabled),
                            bridge_bind: bridge_bind || "0.0.0.0:8787",
                            bridge_token: bridge_token ?? "",
                        },
                        cloudService:
                            selected_cloud_service === "custom_api" ? "custom_api" : "dropbox",
                        dropbox: {
                            db_app_key: db_app_key ?? "",
                            db_app_secret: db_app_secret ?? "",
                        },
                        customApi: {
                            custom_api_url: custom_api_url ?? "",
                            custom_api_bearer_token: custom_api_bearer_token ?? "",
                            aero_customer_base_url: aero_customer_base_url ?? "",
                            aero_customer_api_token: aero_customer_api_token ?? "",
                            custom_api_upload_endpoint: custom_api_upload_endpoint || "/upload",
                            custom_api_share_endpoint: custom_api_share_endpoint || "/share",
                            custom_api_health_endpoint: custom_api_health_endpoint || "/health",
                            custom_api_upload_mode: normalizeCustomApiUploadMode(custom_api_upload_mode),
                            custom_db_app_key: custom_db_app_key ?? "",
                            custom_db_app_secret: custom_db_app_secret ?? "",
                        },
                        email: {
                            smtp_host,
                            smtp_port,
                            smtp_user: smtp_user ?? "",
                            smtp_pass: smtp_pass ?? "",
                            smtp_sender_addr,
                            smtp_sender_name,
                            smtp_fallback_recipient,
                            smtp_sandbox_mode: boolFromSetting(smtp_sandbox_mode),
                            imap_save_sent_enabled: boolFromSetting(imap_save_sent_enabled, true),
                            imap_host,
                            imap_port,
                            imap_sent_folder,
                            imap_same_credentials: boolFromSetting(imap_same_credentials, true),
                            imap_user: imap_user ?? "",
                            imap_pass: imap_pass ?? "",
                        },
                        sms: {
                            seven_api_key: seven_api_key ?? "",
                            seven_sandbox_api_key: seven_sandbox_api_key ?? "",
                            seven_sender,
                            seven_sandbox_mode: boolFromSetting(seven_sandbox_mode),
                        },
                        shortener: {
                            link_shortener_enabled: boolFromSetting(link_shortener_enabled),
                            shortener_base_url: base,
                            shortener_api_key: shortener_api_key || skylink_api_key || "",
                            shortener_expires_preset: shortener_expires_preset || "permanent",
                        },
                        whatsapp: {
                            twilio_account_sid: twilio_account_sid ?? "",
                            twilio_auth_token: twilio_auth_token ?? "",
                            twilio_whatsapp_from,
                        },
                    }),
                );

                const sandbox = boolFromSetting(seven_sandbox_mode);
                const balanceKey = sandbox ? seven_sandbox_api_key : seven_api_key;
                setDbStatusLoading(true);
                void verifyDropboxStatus("native")
                    .then((nativeStatus) => {
                        setDbStatus(nativeStatus);
                    })
                    .catch(() => {
                        setDbStatus("Verbindungsfehler");
                    })
                    .finally(() => {
                        setDbStatusLoading(false);
                    });

                setCustomDbStatusLoading(true);
                void verifyDropboxStatus("custom")
                    .then((customStatus) => {
                        setCustomDbStatus(customStatus);
                    })
                    .catch(() => {
                        setCustomDbStatus("Verbindungsfehler");
                    })
                    .finally(() => {
                        setCustomDbStatusLoading(false);
                    });

                if (balanceKey) {
                    setSmsBalanceLoading(true);
                    setSmsBalance("Unbekannt");
                    void getSmsBalance(balanceKey)
                        .then((balance) => {
                            setSmsBalance(balance);
                        })
                        .catch(() => {
                            setSmsBalance("Netzwerkfehler");
                        })
                        .finally(() => {
                            setSmsBalanceLoading(false);
                        });
                } else {
                    setSmsBalance("Fehlender API-Key");
                    setSmsBalanceLoading(false);
                }
            } catch (err) {
                setLoadError(
                    `Geheimnisse konnten nicht geladen werden (Keyring): ${err}. ` +
                    "API-URLs und Tokens werden ggf. leer angezeigt — bitte nicht speichern, sonst bleiben sie leer.",
                );
            }
        } catch (err) {
            setLoadError(`Einstellungen konnten nicht geladen werden: ${err}`);
        }
    }, []);

    useEffect(() => {
        if (!isOpen) return;
        void loadAll();
    }, [isOpen, loadAll]);

    useEffect(() => {
        if (!isOpen || tab !== "general") return;
        void loadAtsHosts();
        const timer = window.setInterval(() => {
            void loadAtsHosts();
        }, 5000);
        return () => window.clearInterval(timer);
    }, [isOpen, tab, loadAtsHosts]);

    useEffect(() => {
        if (!isOpen || tab !== "general" || !selectedAtsHostId) {
            setSelectedAtsDetails(null);
            return;
        }
        void loadAtsDetails(selectedAtsHostId);
    }, [isOpen, tab, selectedAtsHostId, loadAtsDetails]);

    async function saveAll(event: FormEvent) {
        event.preventDefault();
        setBusy(true);
        setLoadError("");
        let bridgeWarning: string | null = null;
        try {
            const interval = Number.parseInt(general.scan_interval, 10);
            const scan = Number.isFinite(interval)
                ? String(Math.min(3600, Math.max(5, interval)))
                : "10";
            const stabilitySecs = Number.parseInt(general.folder_stability_seconds, 10);
            const stability = Number.isFinite(stabilitySecs)
                ? String(Math.min(3600, Math.max(0, stabilitySecs)))
                : "15";
            const monitorChanged = monitorSettingsChanged(
                savedSnapshot,
                general,
                scan,
                stability,
            );

            await saveSetting("monitor_path", general.monitor_path.trim());
            await saveSetting("archive_path", general.archive_path.trim());
            await saveSetting("log_file_path", general.log_file_path.trim());
            await saveSetting("scan_interval", scan);
            await saveSetting(
                "folder_stability_enabled",
                general.folder_stability_enabled ? "true" : "false",
            );
            await saveSetting("folder_stability_seconds", stability);
            await saveSetting(
                "bridge_enabled",
                general.bridge_enabled ? "true" : "false",
            );
            await saveSetting(
                "bridge_bind",
                general.bridge_bind.trim() || "0.0.0.0:8787",
            );
            await saveSetting("ui_theme", themeMode);
            await saveSetting("selected_cloud_service", cloudService);
            await saveSetting(
                "custom_api_upload_endpoint",
                customApi.custom_api_upload_endpoint.trim() || "/upload",
            );
            await saveSetting(
                "custom_api_share_endpoint",
                customApi.custom_api_share_endpoint.trim() || "/share",
            );
            await saveSetting(
                "custom_api_health_endpoint",
                customApi.custom_api_health_endpoint.trim() || "/health",
            );
            await saveSetting(
                "custom_api_upload_mode",
                normalizeCustomApiUploadMode(customApi.custom_api_upload_mode),
            );
            await saveSetting("smtp_host", email.smtp_host.trim());
            await saveSetting("smtp_port", email.smtp_port.trim() || "587");
            await saveSetting("smtp_sender_addr", email.smtp_sender_addr.trim());
            await saveSetting("smtp_sender_name", email.smtp_sender_name.trim());
            await saveSetting("smtp_fallback_recipient", email.smtp_fallback_recipient.trim());
            await saveSetting("smtp_sandbox_mode", email.smtp_sandbox_mode ? "true" : "false");
            await saveSetting(
                "imap_save_sent_enabled",
                email.imap_save_sent_enabled ? "true" : "false",
            );
            await saveSetting("imap_host", email.imap_host.trim());
            await saveSetting("imap_port", email.imap_port.trim() || "993");
            await saveSetting("imap_sent_folder", email.imap_sent_folder.trim());
            await saveSetting(
                "imap_same_credentials",
                email.imap_same_credentials ? "true" : "false",
            );
            await saveSetting("seven_sender", sms.seven_sender.trim());
            await saveSetting("seven_sandbox_mode", sms.seven_sandbox_mode ? "true" : "false");
            await saveSetting(
                "link_shortener_enabled",
                shortener.link_shortener_enabled ? "true" : "false",
            );
            await saveSetting("shortener_expires_preset", shortener.shortener_expires_preset);
            await saveSetting("twilio_whatsapp_from", whatsapp.twilio_whatsapp_from.trim());

            await persistSecret("db_app_key", dropbox.db_app_key);
            await persistSecret("db_app_secret", dropbox.db_app_secret);
            await persistSecret("custom_api_url", customApi.custom_api_url);
            await persistSecret("custom_api_bearer_token", customApi.custom_api_bearer_token);
            await persistSecret("aero_customer_base_url", customApi.aero_customer_base_url);
            await persistSecret("aero_customer_api_token", customApi.aero_customer_api_token);
            await persistSecret("custom_db_app_key", customApi.custom_db_app_key);
            await persistSecret("custom_db_app_secret", customApi.custom_db_app_secret);
            await persistSecret("smtp_user", email.smtp_user);
            // Passwords may intentionally contain leading/trailing spaces — only skip if blank.
            if (email.smtp_pass.length > 0) await saveSecret("smtp_pass", email.smtp_pass);
            await persistSecret("imap_user", email.imap_user);
            if (email.imap_pass.length > 0) await saveSecret("imap_pass", email.imap_pass);
            await persistSecret("seven_api_key", sms.seven_api_key);
            await persistSecret("seven_sandbox_api_key", sms.seven_sandbox_api_key);
            await persistSecret("shortener_base_url", shortener.shortener_base_url);
            await persistSecret("shortener_api_key", shortener.shortener_api_key);
            await persistSecret("twilio_account_sid", whatsapp.twilio_account_sid);
            await persistSecret("twilio_auth_token", whatsapp.twilio_auth_token);
            await persistSecret("bridge_token", general.bridge_token);

            try {
                const status = await applyBridgeConfig();
                setBridgeStatusLabel(
                    status.running
                        ? `Aktiv auf ${status.bind_addr}`
                        : "Inaktiv",
                );
            } catch (bridgeErr) {
                bridgeWarning = String(bridgeErr);
                setBridgeStatusLabel(`Fehler: ${bridgeErr}`);
            }

            const savedGeneral = {
                ...general,
                scan_interval: scan,
                folder_stability_seconds: stability,
            };
            setGeneral(savedGeneral);
            setSavedSnapshot(
                captureSettingsFormSnapshot({
                    themeMode,
                    general: savedGeneral,
                    cloudService,
                    dropbox,
                    customApi,
                    email,
                    sms,
                    shortener,
                    whatsapp,
                }),
            );

            let successMessage = "Einstellungen gespeichert.";
            if (monitorChanged) {
                successMessage +=
                    "\n\nMonitor-Einstellungen gelten ab dem nächsten Scan.";
            }
            showAppToast(successMessage, {
                tone: "success",
                title: "Einstellungen",
                id: "settings-save",
            });
            onClose();
            if (bridgeWarning) {
                showAppToast(
                    `Einstellungen gespeichert, aber die Bridge konnte nicht gestartet werden:\n${bridgeWarning}`,
                    {
                        tone: "warning",
                        title: "Bridge",
                        durationMs: 7000,
                        id: "settings-bridge-warning",
                    },
                );
            }
        } catch (err) {
            showAppToast(String(err), {
                tone: "error",
                title: "Speichern fehlgeschlagen",
                id: "settings-save-error",
            });
        } finally {
            setBusy(false);
        }
    }

    async function toggleDropbox() {
        setDbBusy(true);
        try {
            await persistSecret("db_app_key", dropbox.db_app_key);
            await persistSecret("db_app_secret", dropbox.db_app_secret);
            if (dbStatus === "Verbunden") {
                const result = await disconnectDropbox("native");
                setDbStatus(result.status);
                return;
            }
            setDbStatus("Warte auf OAuth...");
            let result = await connectDropbox("native");
            if (result.needs_oauth && result.authorize_url && result.code_verifier) {
                const code = await promptAuthCode(result.authorize_url);
                if (!code) {
                    setDbStatus("Nicht verbunden (Abbruch)");
                    return;
                }
                result = await finishDropboxOauth("native", code, result.code_verifier);
            }
            setDbStatus(result.status);
            if (!result.success) {
                showError(result.message || "Dropbox-Verbindung fehlgeschlagen.", "Dropbox");
            } else {
                showAppToast(result.message || "Verbunden.", {
                    tone: "success",
                    title: "Dropbox",
                });
            }
        } catch (err) {
            setDbStatus("Verbindungsfehler");
            showError(String(err), "Dropbox");
        } finally {
            setDbBusy(false);
        }
    }

    async function toggleCustomDropbox() {
        setCustomBusy(true);
        try {
            await persistSecret("custom_db_app_key", customApi.custom_db_app_key);
            await persistSecret("custom_db_app_secret", customApi.custom_db_app_secret);
            if (customDbStatus === "Verbunden" || customDbStatus.startsWith("Verbunden")) {
                const result = await disconnectDropbox("custom");
                setCustomDbStatus(result.status);
                return;
            }
            setCustomDbStatus("Warte auf OAuth...");
            let result = await connectDropbox("custom");
            if (result.needs_oauth && result.authorize_url && result.code_verifier) {
                const code = await promptAuthCode(result.authorize_url);
                if (!code) {
                    setCustomDbStatus("Nicht verbunden (Abbruch)");
                    return;
                }
                result = await finishDropboxOauth("custom", code, result.code_verifier);
            }
            setCustomDbStatus(result.status);
            if (!result.success) {
                showError(
                    result.message || "Custom-Dropbox-Verbindung fehlgeschlagen.",
                    "Custom Dropbox",
                );
            } else {
                showAppToast(result.message || "Verbunden.", {
                    tone: "success",
                    title: "Custom Dropbox",
                });
            }
        } catch (err) {
            setCustomDbStatus("Verbindungsfehler");
            showError(String(err), "Custom Dropbox");
        } finally {
            setCustomBusy(false);
        }
    }

    async function toggleCustomApi() {
        setCustomBusy(true);
        try {
            await persistSecret("custom_api_url", customApi.custom_api_url);
            await persistSecret("custom_api_bearer_token", customApi.custom_api_bearer_token);
            await persistSecret("aero_customer_base_url", customApi.aero_customer_base_url);
            await persistSecret("aero_customer_api_token", customApi.aero_customer_api_token);
            await saveSetting(
                "custom_api_upload_endpoint",
                customApi.custom_api_upload_endpoint.trim() || "/upload",
            );
            await saveSetting(
                "custom_api_share_endpoint",
                customApi.custom_api_share_endpoint.trim() || "/share",
            );
            await saveSetting(
                "custom_api_health_endpoint",
                customApi.custom_api_health_endpoint.trim() || "/health",
            );
            await saveSetting(
                "custom_api_upload_mode",
                normalizeCustomApiUploadMode(customApi.custom_api_upload_mode),
            );

            if (customApiStatus === "Verbunden") {
                const result = await disconnectCustomApi();
                setCustomApiStatus(result.status);
                return;
            }
            setCustomApiStatus("Teste Verbindung...");
            const result = await connectCustomApi();
            setCustomApiStatus(result.status);
            if (result.success) {
                showSuccess(
                    "Die Verbindung zur Custom API wurde erfolgreich getestet!",
                    "Custom API",
                );
            } else {
                showError(
                    result.message || "Custom-API-Verbindung fehlgeschlagen.",
                    "Custom API",
                );
            }
        } catch (err) {
            setCustomApiStatus("Verbindungsfehler");
            showError(String(err), "Custom API");
        } finally {
            setCustomBusy(false);
        }
    }

    async function refreshBalance() {
        const key = sms.seven_sandbox_mode ? sms.seven_sandbox_api_key : sms.seven_api_key;
        if (!key.trim()) {
            setSmsBalance("Fehlender API-Key");
            return;
        }
        setSmsBusy(true);
        setSmsBalance("Lade...");
        try {
            const bal = await getSmsBalance(key.trim());
            setSmsBalance(bal);
        } catch {
            setSmsBalance("Netzwerkfehler");
        } finally {
            setSmsBusy(false);
        }
    }

    async function onTestShortener() {
        setShortenerBusy(true);
        try {
            const short = await testLinkShortener(
                shortener.shortener_base_url,
                shortener.shortener_api_key,
                shortener.shortener_expires_preset,
            );
            showSuccess(`Test erfolgreich.\n\nKurzlink:\n${short}`, "Link-Shortener");
        } catch (err) {
            showError(String(err), "Link-Shortener");
        } finally {
            setShortenerBusy(false);
        }
    }

    return (
        <Dialog
            open={isOpen}
            onOpenChange={(v) => {
                if (!v) void requestClose();
            }}
        >
            <DialogContent
                className="relative flex h-[min(85vh,42rem)] max-w-2xl flex-col gap-4 overflow-visible"
                onPointerDownOutside={(e) => {
                    if (busy) e.preventDefault();
                }}
                onEscapeKeyDown={(e) => {
                    if (busy) e.preventDefault();
                }}
            >
                <DialogHeader className="shrink-0">
                    <DialogTitle>Einstellungen</DialogTitle>
                    <DialogDescription className="sr-only">
                        App-Einstellungen bearbeiten
                    </DialogDescription>
                </DialogHeader>

                <form
                    className="flex min-h-0 flex-1 flex-col gap-4 overflow-hidden"
                    onSubmit={saveAll}
                >
                    <Tabs
                        value={tab}
                        onValueChange={(v) => setTab(v as TabId)}
                        className="flex min-h-0 flex-1 flex-col overflow-hidden"
                    >
                        <TabsList className="flex h-auto shrink-0 flex-wrap gap-1">
                            {TAB_ITEMS.map(({id, label}) => (
                                <TabsTrigger key={id} value={id}>
                                    {label}
                                </TabsTrigger>
                            ))}
                        </TabsList>

                        <div className="min-h-0 flex-1 overflow-y-auto px-1 py-1 pr-2 [scrollbar-gutter:stable]">
                            <TabsContent value="general" className="mt-4 space-y-4">
                                <SettingsSection title="Ordner & Scan">
                                    <div className="space-y-3">
                                        <PathField
                                            label="Zu überwachender Ordner"
                                            value={general.monitor_path}
                                            onChange={(v) =>
                                                setGeneral((p) => ({...p, monitor_path: v}))
                                            }
                                            onPick={() => {
                                                void pickDirectory(general.monitor_path).then((dir) => {
                                                    if (dir) setGeneral((p) => ({...p, monitor_path: dir}));
                                                });
                                            }}
                                        />
                                        <PathField
                                            label="Archiv-Ordner"
                                            value={general.archive_path}
                                            onChange={(v) =>
                                                setGeneral((p) => ({...p, archive_path: v}))
                                            }
                                            onPick={() => {
                                                void pickDirectory(general.archive_path).then((dir) => {
                                                    if (dir) setGeneral((p) => ({...p, archive_path: dir}));
                                                });
                                            }}
                                        />
                                        <PathField
                                            label="Log-Datei-Ordner"
                                            value={general.log_file_path}
                                            placeholder="Leer = App-Datenverzeichnis"
                                            onChange={(v) =>
                                                setGeneral((p) => ({...p, log_file_path: v}))
                                            }
                                            onPick={() => {
                                                void pickDirectory(general.log_file_path).then((dir) => {
                                                    if (dir) setGeneral((p) => ({...p, log_file_path: dir}));
                                                });
                                            }}
                                        />
                                        <Field label="Scan-Intervall (Sekunden)">
                                            <Input
                                                type="number"
                                                min={5}
                                                max={3600}
                                                value={general.scan_interval}
                                                onChange={(e) =>
                                                    setGeneral((p) => ({
                                                        ...p,
                                                        scan_interval: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={general.folder_stability_enabled}
                                                onCheckedChange={(v) =>
                                                    setGeneral((p) => ({
                                                        ...p,
                                                        folder_stability_enabled: v === true,
                                                    }))
                                                }
                                            />
                                            Ordner-Stabilität prüfen
                                        </label>
                                        <Field label="Stabilität (Sekunden unverändert)">
                                            <Input
                                                type="number"
                                                min={0}
                                                max={3600}
                                                value={general.folder_stability_seconds}
                                                disabled={!general.folder_stability_enabled}
                                                onChange={(e) =>
                                                    setGeneral((p) => ({
                                                        ...p,
                                                        folder_stability_seconds: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                    </div>
                                </SettingsSection>

                                <SettingsSection
                                    title="LAN-Bridge (ATS)"
                                    description="Optionaler Control-Plane-Server im LAN (inkl. mDNS-Advertise _ams-bridge._tcp). Datei-Handoff funktioniert auch ohne Bridge."
                                >
                                    <div className="space-y-3">
                                        <div className="flex flex-wrap items-center justify-between gap-2">
                                            <div className="inline-flex rounded-lg border border-border/60 bg-muted/20 p-1">
                                                <button
                                                    type="button"
                                                    className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${
                                                        bridgeView === "monitoring"
                                                            ? "bg-background text-foreground shadow-sm"
                                                            : "text-muted hover:text-foreground"
                                                    }`}
                                                    onClick={() => setBridgeView("monitoring")}
                                                >
                                                    Monitoring
                                                </button>
                                                <button
                                                    type="button"
                                                    className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${
                                                        bridgeView === "clients"
                                                            ? "bg-background text-foreground shadow-sm"
                                                            : "text-muted hover:text-foreground"
                                                    }`}
                                                    onClick={() => setBridgeView("clients")}
                                                >
                                                    ATS-Clients ({atsHostsLoading ? "…" : connectedAtsHostsCount})
                                                </button>
                                            </div>
                                            {bridgeView === "clients" ? (
                                                <Button
                                                    type="button"
                                                    variant="secondary"
                                                    size="sm"
                                                    disabled={atsHostsLoading}
                                                    onClick={() => void loadAtsHosts()}
                                                >
                                                    Aktualisieren
                                                </Button>
                                            ) : null}
                                        </div>

                                        {bridgeView === "monitoring" ? (
                                            <div className="space-y-3">
                                                <label className="flex items-center gap-2 text-sm">
                                                    <Checkbox
                                                        checked={general.bridge_enabled}
                                                        onCheckedChange={(v) =>
                                                            setGeneral((p) => ({
                                                                ...p,
                                                                bridge_enabled: v === true,
                                                            }))
                                                        }
                                                    />
                                                    Bridge-Server aktivieren
                                                </label>
                                                <Field label="Bind-Adresse (LAN, z. B. 0.0.0.0:8787)">
                                                    <Input
                                                        value={general.bridge_bind}
                                                        disabled={!general.bridge_enabled}
                                                        onChange={(e) =>
                                                            setGeneral((p) => ({
                                                                ...p,
                                                                bridge_bind: e.target.value,
                                                            }))
                                                        }
                                                        placeholder="0.0.0.0:8787"
                                                    />
                                                </Field>
                                                <Field label="Bridge-Token (Pflicht bei aktivierter Bridge)">
                                                    <PasswordInput
                                                        value={general.bridge_token}
                                                        disabled={!general.bridge_enabled}
                                                        onChange={(e) =>
                                                            setGeneral((p) => ({
                                                                ...p,
                                                                bridge_token: e.target.value,
                                                            }))
                                                        }
                                                        autoComplete="off"
                                                    />
                                                </Field>
                                                <div className="flex flex-wrap items-center gap-2">
                                                    <Button
                                                        type="button"
                                                        variant="secondary"
                                                        size="sm"
                                                        disabled={bridgeBusy}
                                                        onClick={() => {
                                                            void (async () => {
                                                                setBridgeBusy(true);
                                                                try {
                                                                    await saveSetting(
                                                                        "bridge_enabled",
                                                                        general.bridge_enabled ? "true" : "false",
                                                                    );
                                                                    await saveSetting(
                                                                        "bridge_bind",
                                                                        general.bridge_bind.trim() || "0.0.0.0:8787",
                                                                    );
                                                                    await persistSecret(
                                                                        "bridge_token",
                                                                        general.bridge_token,
                                                                    );
                                                                    const status = await applyBridgeConfig();
                                                                    setBridgeStatusLabel(
                                                                        status.running
                                                                            ? `Aktiv auf ${status.bind_addr}`
                                                                            : "Inaktiv",
                                                                    );
                                                                    showAppToast(
                                                                        status.running
                                                                            ? `Bridge gestartet: ${status.bind_addr}`
                                                                            : "Bridge gestoppt.",
                                                                        {
                                                                            tone: "success",
                                                                            title: "Bridge",
                                                                        },
                                                                    );
                                                                    setSavedSnapshot((prev) =>
                                                                        prev
                                                                            ? {
                                                                                ...prev,
                                                                                general: {
                                                                                    ...prev.general,
                                                                                    bridge_enabled:
                                                                                        general.bridge_enabled,
                                                                                    bridge_bind:
                                                                                        general.bridge_bind.trim() ||
                                                                                        "0.0.0.0:8787",
                                                                                    bridge_token:
                                                                                        general.bridge_token,
                                                                                },
                                                                            }
                                                                            : prev,
                                                                    );
                                                                } catch (err) {
                                                                    setBridgeStatusLabel(`Fehler: ${err}`);
                                                                    showAppToast(String(err), {
                                                                        tone: "error",
                                                                        title: "Bridge",
                                                                    });
                                                                } finally {
                                                                    setBridgeBusy(false);
                                                                }
                                                            })();
                                                        }}
                                                    >
                                                        {bridgeBusy ? "…" : "Bridge anwenden"}
                                                    </Button>
                                                    <InlineStatus
                                                        label="Status"
                                                        value={bridgeStatusLabel}
                                                        loading={bridgeStatusLoading}
                                                    />
                                                </div>
                                            </div>
                                        ) : (
                                            <div className="space-y-3">
                                                <div className="grid gap-3 sm:grid-cols-3">
                                                    <div className="rounded-lg border border-border/60 bg-muted/15 p-3">
                                                        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
                                                            Verbundene Clients
                                                        </p>
                                                        <p className="mt-1 text-2xl font-semibold text-foreground">
                                                            {atsHostsLoading ? "…" : connectedAtsHostsCount}
                                                        </p>
                                                    </div>
                                                    <div className="rounded-lg border border-border/60 bg-muted/15 p-3">
                                                        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
                                                            Aktiv
                                                        </p>
                                                        <p className="mt-1 text-2xl font-semibold text-foreground">
                                                            {atsHostsLoading ? "…" : activeAtsHostsCount}
                                                        </p>
                                                    </div>
                                                    <div className="rounded-lg border border-border/60 bg-muted/15 p-3">
                                                        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted">
                                                            Sichtbarkeit
                                                        </p>
                                                        <p className="mt-1 text-sm text-muted">
                                                            Verbundene ~2 Min. · Aktiv 60 Min. · Inaktiv &gt;30 Tage
                                                        </p>
                                                    </div>
                                                </div>

                                                {atsHostsError ? (
                                                    <p className="text-xs text-destructive">{atsHostsError}</p>
                                                ) : null}

                                                <div className="grid gap-3 xl:grid-cols-[minmax(0,0.95fr)_minmax(0,1.25fr)]">
                                                    <div className="min-w-0 max-h-[min(52vh,28rem)] overflow-y-auto pr-1 [scrollbar-gutter:stable]">
                                                        <AtsHostListSections
                                                            hosts={atsHosts}
                                                            selectedHostId={selectedAtsHostId}
                                                            onSelectHost={setSelectedAtsHostId}
                                                        />
                                                    </div>

                                                    <div className="min-w-0 rounded-lg border border-border/60 bg-muted/10 p-4">
                                                        {!selectedAtsHost ? (
                                                            <div className="text-sm text-muted">
                                                                Client auswählen, um letzte Events und Vorgänge zu sehen.
                                                            </div>
                                                        ) : atsDetailsLoading ? (
                                                            <div className="flex items-center gap-2 text-sm text-muted">
                                                                <Spinner size={14} className="border-[1.5px]"/>
                                                                Details werden geladen…
                                                            </div>
                                                        ) : !selectedAtsDetails ? (
                                                            <div className="text-sm text-muted">
                                                                Für diesen Client sind derzeit keine Details verfügbar.
                                                            </div>
                                                        ) : (
                                                            <div className="space-y-4">
                                                                <div className="space-y-3">
                                                                    <div className="flex flex-wrap items-center gap-2">
                                                                        <span className="text-base font-semibold text-foreground">
                                                                            {selectedAtsDetails.host.hostname}
                                                                        </span>
                                                                        <AtsPresenceChip
                                                                            label={
                                                                                selectedAtsHost
                                                                                    ? atsPresenceChipLabel(selectedAtsHost)
                                                                                    : "—"
                                                                            }
                                                                            tone={
                                                                                selectedAtsHost
                                                                                    ? atsPresenceChipTone(selectedAtsHost)
                                                                                    : "inactive"
                                                                            }
                                                                        />
                                                                    </div>
                                                                    <div className="grid gap-2 sm:grid-cols-2">
                                                                        <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs text-muted">
                                                                            <p className="font-medium text-foreground">
                                                                                Client
                                                                            </p>
                                                                            <p className="mt-1">
                                                                                {selectedAtsDetails.host.ats_app || "—"} {selectedAtsDetails.host.ats_version || ""}
                                                                            </p>
                                                                            <p className="mt-1 truncate">
                                                                                {selectedAtsDetails.host.instance_id}
                                                                            </p>
                                                                        </div>
                                                                        <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs text-muted">
                                                                            <p className="font-medium text-foreground">
                                                                                Sichtbarkeit
                                                                            </p>
                                                                            <p className="mt-1">
                                                                                First seen: {formatTimestamp(selectedAtsDetails.host.first_seen_at)}
                                                                            </p>
                                                                            <p className="mt-1">
                                                                                Last seen: {formatTimestamp(selectedAtsDetails.host.last_seen_at)}
                                                                            </p>
                                                                        </div>
                                                                    </div>
                                                                    <div className="rounded-md border border-border/50 bg-background/80 p-3 text-xs text-muted">
                                                                        Letztes Event: {eventTypeLabel(selectedAtsDetails.host.last_event_type)} um{" "}
                                                                        {formatTimestamp(selectedAtsDetails.host.last_event_at)}
                                                                    </div>
                                                                </div>

                                                                <div className="space-y-2">
                                                                    <p className="text-xs font-semibold uppercase tracking-wide text-muted">
                                                                        Letzte Events
                                                                    </p>
                                                                    {selectedAtsDetails.host.recent_events.length === 0 ? (
                                                                        <p className="text-sm text-muted">Keine Events im Zeitfenster.</p>
                                                                    ) : (
                                                                        <div className="space-y-2">
                                                                            {selectedAtsDetails.host.recent_events.slice(0, 8).map((entry) => (
                                                                                <AtsActivityEventCard
                                                                                    key={`${entry.occurred_at}-${entry.event_type}-${entry.correlation_id}-${entry.payload_json.slice(0, 24)}`}
                                                                                    entry={entry}
                                                                                />
                                                                            ))}
                                                                        </div>
                                                                    )}
                                                                </div>

                                                                <div className="space-y-2">
                                                                    <p className="text-xs font-semibold uppercase tracking-wide text-muted">
                                                                        Letzte Vorgänge
                                                                    </p>
                                                                    {selectedAtsDetails.recent_jobs.length === 0 ? (
                                                                        <p className="text-sm text-muted">
                                                                            Keine korrelierten Vorgänge im Zeitraum.
                                                                        </p>
                                                                    ) : (
                                                                        <div className="space-y-2">
                                                                            {selectedAtsDetails.recent_jobs.slice(0, 8).map((job) => (
                                                                                <div
                                                                                    key={job.correlation_id}
                                                                                    className="rounded-md border border-border/50 bg-background/80 p-3 text-xs"
                                                                                >
                                                                                    <div
                                                                                        className="flex flex-wrap items-center justify-between gap-2">
                                                                                        <span className="truncate font-medium text-foreground">
                                                                                            {job.folder_name || "Ohne Ordnername"}
                                                                                        </span>
                                                                                        <StatusChip
                                                                                            status={job.ams_status_label}
                                                                                            channel="overall"
                                                                                            compact
                                                                                        />
                                                                                    </div>
                                                                                    <div className="mt-2 space-y-0.5 text-muted">
                                                                                        <p>Correlation ID: {job.correlation_id}</p>
                                                                                        <p>Quelle: {eventTypeLabel(job.source_event_type)}</p>
                                                                                        <p>Last seen: {formatTimestamp(job.last_seen_at)}</p>
                                                                                    </div>
                                                                                </div>
                                                                            ))}
                                                                        </div>
                                                                    )}
                                                                </div>
                                                            </div>
                                                        )}
                                                    </div>
                                                </div>
                                            </div>
                                        )}
                                    </div>
                                </SettingsSection>

                                <SettingsSection
                                    title="Darstellung"
                                    description="Hell- oder Dunkelmodus der Oberfläche."
                                >
                                    <div className="flex flex-wrap gap-2">
                                        {(["dark", "light"] as ThemeMode[]).map((mode) => (
                                            <Button
                                                key={mode}
                                                type="button"
                                                variant={themeMode === mode ? "default" : "secondary"}
                                                onClick={() => {
                                                    setThemeMode(mode);
                                                    void saveSetting("ui_theme", mode);
                                                    setSavedSnapshot((prev) =>
                                                        prev ? {...prev, themeMode: mode} : prev,
                                                    );
                                                }}
                                            >
                                                {mode === "dark" ? (
                                                    <Moon className="h-4 w-4"/>
                                                ) : (
                                                    <Sun className="h-4 w-4"/>
                                                )}
                                                {mode === "dark" ? "Dunkel" : "Hell"}
                                            </Button>
                                        ))}
                                    </div>
                                </SettingsSection>
                            </TabsContent>

                            <TabsContent value="cloud" className="mt-4 space-y-4">
                                <SettingsSection title="Aktiver Cloud-Dienst">
                                    <div className="flex flex-wrap gap-2">
                                        <Button
                                            type="button"
                                            variant={cloudService === "dropbox" ? "default" : "secondary"}
                                            onClick={() => setCloudService("dropbox")}
                                        >
                                            Dropbox
                                        </Button>
                                        <Button
                                            type="button"
                                            variant={
                                                cloudService === "custom_api" ? "default" : "secondary"
                                            }
                                            onClick={() => setCloudService("custom_api")}
                                        >
                                            Custom API
                                        </Button>
                                    </div>
                                </SettingsSection>

                                {cloudService === "dropbox" ? (
                                    <SettingsSection title="Dropbox">
                                        <div className="space-y-3">
                                            <Field label="App Key">
                                                <PasswordInput
                                                    autoComplete="off"
                                                    value={dropbox.db_app_key}
                                                    disabled={dbStatus === "Verbunden"}
                                                    onChange={(e) =>
                                                        setDropbox((p) => ({
                                                            ...p,
                                                            db_app_key: e.target.value,
                                                        }))
                                                    }
                                                />
                                            </Field>
                                            <Field label="App Secret">
                                                <PasswordInput
                                                    autoComplete="off"
                                                    value={dropbox.db_app_secret}
                                                    disabled={dbStatus === "Verbunden"}
                                                    onChange={(e) =>
                                                        setDropbox((p) => ({
                                                            ...p,
                                                            db_app_secret: e.target.value,
                                                        }))
                                                    }
                                                />
                                            </Field>
                                            <InlineStatus
                                                label="Status"
                                                value={dbStatus}
                                                loading={dbStatusLoading}
                                            />
                                            <Button
                                                type="button"
                                                disabled={
                                                    dbBusy ||
                                                    (dbStatus !== "Verbunden" &&
                                                        (!dropbox.db_app_key.trim() ||
                                                            !dropbox.db_app_secret.trim()))
                                                }
                                                onClick={() => void toggleDropbox()}
                                            >
                                                {dbStatus === "Verbunden"
                                                    ? "Verbindung trennen"
                                                    : "Mit Dropbox verbinden"}
                                            </Button>
                                        </div>
                                    </SettingsSection>
                                ) : (
                                    <>
                                        <SettingsSection title="Custom API">
                                            <div className="space-y-3">
                                                <Field label="API-URL">
                                                    <Input
                                                        value={customApi.custom_api_url}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_api_url: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Bearer Token">
                                                    <PasswordInput
                                                        autoComplete="off"
                                                        value={customApi.custom_api_bearer_token}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_api_bearer_token: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Customer Base URL">
                                                    <Input
                                                        value={customApi.aero_customer_base_url}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                aero_customer_base_url: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Customer API Token">
                                                    <PasswordInput
                                                        autoComplete="off"
                                                        value={customApi.aero_customer_api_token}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                aero_customer_api_token: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Upload-Endpoint">
                                                    <Input
                                                        value={customApi.custom_api_upload_endpoint}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_api_upload_endpoint: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Share-Endpoint">
                                                    <Input
                                                        value={customApi.custom_api_share_endpoint}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_api_share_endpoint: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Health-Endpoint">
                                                    <Input
                                                        value={customApi.custom_api_health_endpoint}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_api_health_endpoint: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="Upload-Modus">
                                                    <Select
                                                        value={normalizeCustomApiUploadMode(
                                                            customApi.custom_api_upload_mode,
                                                        )}
                                                        onValueChange={(v) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_api_upload_mode: normalizeCustomApiUploadMode(v),
                                                            }))
                                                        }
                                                    >
                                                        <SelectTrigger>
                                                            <SelectValue/>
                                                        </SelectTrigger>
                                                        <SelectContent>
                                                            <SelectItem value={CUSTOM_API_UPLOAD_MODE_PROXIED}>
                                                                Proxy Session Upload
                                                            </SelectItem>
                                                            <SelectItem value={CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX}>
                                                                Dropbox Upload + Manifest v1.1
                                                            </SelectItem>
                                                        </SelectContent>
                                                    </Select>
                                                </Field>
                                                {normalizeCustomApiUploadMode(customApi.custom_api_upload_mode) ===
                                                    CUSTOM_API_UPLOAD_MODE_DIRECT_DROPBOX && (
                                                        <p className="text-xs text-muted">
                                                            Benötigt das Custom-API-Dropbox-Konto (App Key/Secret +
                                                            OAuth)
                                                            unten. Ohne Verbindung schlägt der Upload fehl.
                                                        </p>
                                                    )}
                                                <p className="text-xs text-muted">
                                                    Status: {customApiStatus}
                                                </p>
                                                <Button
                                                    type="button"
                                                    disabled={
                                                        customBusy ||
                                                        !customApi.custom_api_url.trim() ||
                                                        !customApi.custom_api_bearer_token.trim()
                                                    }
                                                    onClick={() => void toggleCustomApi()}
                                                >
                                                    {customApiStatus === "Verbunden"
                                                        ? "Verbindung trennen"
                                                        : "Custom API verbinden"}
                                                </Button>
                                            </div>
                                        </SettingsSection>

                                        <SettingsSection title="Custom-API Dropbox-Konto">
                                            <div className="space-y-3">
                                                <Field label="App Key">
                                                    <PasswordInput
                                                        autoComplete="off"
                                                        value={customApi.custom_db_app_key}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_db_app_key: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <Field label="App Secret">
                                                    <PasswordInput
                                                        autoComplete="off"
                                                        value={customApi.custom_db_app_secret}
                                                        onChange={(e) =>
                                                            setCustomApi((p) => ({
                                                                ...p,
                                                                custom_db_app_secret: e.target.value,
                                                            }))
                                                        }
                                                    />
                                                </Field>
                                                <InlineStatus
                                                    label="Status"
                                                    value={customDbStatus}
                                                    loading={customDbStatusLoading}
                                                />
                                                <Button
                                                    type="button"
                                                    disabled={
                                                        customBusy ||
                                                        !customApi.custom_db_app_key.trim() ||
                                                        !customApi.custom_db_app_secret.trim()
                                                    }
                                                    onClick={() => void toggleCustomDropbox()}
                                                >
                                                    {customDbStatus.startsWith("Verbunden")
                                                        ? "Dropbox trennen"
                                                        : "Mit Dropbox verbinden"}
                                                </Button>
                                            </div>
                                        </SettingsSection>
                                    </>
                                )}
                            </TabsContent>

                            <TabsContent value="email" className="mt-4 space-y-4">
                                <SettingsSection title="SMTP">
                                    <div className="space-y-3">
                                        <Field label="Host">
                                            <Input
                                                value={email.smtp_host}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, smtp_host: e.target.value}))
                                                }
                                            />
                                        </Field>
                                        <Field label="Port">
                                            <Input
                                                type="number"
                                                min={1}
                                                max={65535}
                                                value={email.smtp_port}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, smtp_port: e.target.value}))
                                                }
                                            />
                                        </Field>
                                        <Field label="Benutzername">
                                            <Input
                                                value={email.smtp_user}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, smtp_user: e.target.value}))
                                                }
                                            />
                                        </Field>
                                        <Field label="Passwort">
                                            <PasswordInput
                                                autoComplete="off"
                                                value={email.smtp_pass}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, smtp_pass: e.target.value}))
                                                }
                                            />
                                        </Field>
                                    </div>
                                </SettingsSection>

                                <SettingsSection title="Absender">
                                    <div className="space-y-3">
                                        <Field label="Absender-Adresse">
                                            <Input
                                                value={email.smtp_sender_addr}
                                                onChange={(e) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        smtp_sender_addr: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="Absender-Name">
                                            <Input
                                                value={email.smtp_sender_name}
                                                onChange={(e) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        smtp_sender_name: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="Fallback-Empfänger">
                                            <Input
                                                value={email.smtp_fallback_recipient}
                                                onChange={(e) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        smtp_fallback_recipient: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={email.smtp_sandbox_mode}
                                                onCheckedChange={(v) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        smtp_sandbox_mode: v === true,
                                                    }))
                                                }
                                            />
                                            Sandbox-Modus (alle Mails an Fallback)
                                        </label>
                                    </div>
                                </SettingsSection>

                                <SettingsSection title="IMAP (Gesendet)">
                                    <div className="space-y-3">
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={email.imap_save_sent_enabled}
                                                onCheckedChange={(v) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        imap_save_sent_enabled: v === true,
                                                    }))
                                                }
                                            />
                                            Versendete E-Mails ablegen
                                        </label>
                                        <Field label="IMAP-Host">
                                            <Input
                                                placeholder="Leer = SMTP-Host"
                                                value={email.imap_host}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, imap_host: e.target.value}))
                                                }
                                            />
                                        </Field>
                                        <Field label="IMAP-Port">
                                            <Input
                                                type="number"
                                                min={1}
                                                max={65535}
                                                value={email.imap_port}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, imap_port: e.target.value}))
                                                }
                                            />
                                        </Field>
                                        <Field label="Gesendet-Ordner">
                                            <Input
                                                placeholder="Leer = Auto"
                                                value={email.imap_sent_folder}
                                                onChange={(e) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        imap_sent_folder: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={email.imap_same_credentials}
                                                onCheckedChange={(v) =>
                                                    setEmail((p) => ({
                                                        ...p,
                                                        imap_same_credentials: v === true,
                                                    }))
                                                }
                                            />
                                            Gleiche Zugangsdaten wie SMTP
                                        </label>
                                        <Field label="IMAP-Benutzername">
                                            <Input
                                                disabled={email.imap_same_credentials}
                                                value={email.imap_user}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, imap_user: e.target.value}))
                                                }
                                            />
                                        </Field>
                                        <Field label="IMAP-Passwort">
                                            <PasswordInput
                                                autoComplete="off"
                                                disabled={email.imap_same_credentials}
                                                value={email.imap_pass}
                                                onChange={(e) =>
                                                    setEmail((p) => ({...p, imap_pass: e.target.value}))
                                                }
                                            />
                                        </Field>
                                    </div>
                                </SettingsSection>
                            </TabsContent>

                            <TabsContent value="sms" className="mt-4 space-y-4">
                                <SettingsSection title="Seven.io">
                                    <div className="space-y-3">
                                        <Field label="Production API Key">
                                            <PasswordInput
                                                autoComplete="off"
                                                value={sms.seven_api_key}
                                                onChange={(e) =>
                                                    setSms((p) => ({
                                                        ...p,
                                                        seven_api_key: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="Sandbox API Key">
                                            <PasswordInput
                                                autoComplete="off"
                                                value={sms.seven_sandbox_api_key}
                                                onChange={(e) =>
                                                    setSms((p) => ({
                                                        ...p,
                                                        seven_sandbox_api_key: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="Absender (max. 11 Zeichen)">
                                            <Input
                                                maxLength={11}
                                                value={sms.seven_sender}
                                                onChange={(e) =>
                                                    setSms((p) => ({
                                                        ...p,
                                                        seven_sender: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={sms.seven_sandbox_mode}
                                                onCheckedChange={(v) =>
                                                    setSms((p) => ({
                                                        ...p,
                                                        seven_sandbox_mode: v === true,
                                                    }))
                                                }
                                            />
                                            Sandbox-Modus
                                        </label>
                                        <div className="flex flex-wrap items-center gap-3">
                                            <InlineStatus
                                                label="Aktuelle Balance"
                                                value={smsBalance}
                                                loading={smsBalanceLoading}
                                            />
                                            <Button
                                                type="button"
                                                variant="secondary"
                                                size="sm"
                                                disabled={smsBusy}
                                                onClick={() => void refreshBalance()}
                                            >
                                                Aktualisieren
                                            </Button>
                                        </div>
                                    </div>
                                </SettingsSection>
                            </TabsContent>

                            <TabsContent value="shortener" className="mt-4 space-y-4">
                                <SettingsSection title="Link-Shortener">
                                    <div className="space-y-3">
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={shortener.link_shortener_enabled}
                                                onCheckedChange={(v) =>
                                                    setShortener((p) => ({
                                                        ...p,
                                                        link_shortener_enabled: v === true,
                                                    }))
                                                }
                                            />
                                            Link-Shortener aktivieren
                                        </label>
                                        <Field label="Basis-URL">
                                            <Input
                                                placeholder="https://skydive-media.de"
                                                value={shortener.shortener_base_url}
                                                onChange={(e) =>
                                                    setShortener((p) => ({
                                                        ...p,
                                                        shortener_base_url: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="API-Key">
                                            <PasswordInput
                                                autoComplete="off"
                                                placeholder="key_…"
                                                value={shortener.shortener_api_key}
                                                onChange={(e) =>
                                                    setShortener((p) => ({
                                                        ...p,
                                                        shortener_api_key: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="Gültigkeit ab Erstellung">
                                            <Select
                                                value={shortener.shortener_expires_preset}
                                                onValueChange={(v) =>
                                                    setShortener((p) => ({
                                                        ...p,
                                                        shortener_expires_preset: v,
                                                    }))
                                                }
                                            >
                                                <SelectTrigger>
                                                    <SelectValue/>
                                                </SelectTrigger>
                                                <SelectContent>
                                                    {SHORTENER_PRESETS.map((p) => (
                                                        <SelectItem key={p.value} value={p.value}>
                                                            {p.label}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </Field>
                                        <Button
                                            type="button"
                                            variant="secondary"
                                            disabled={shortenerBusy}
                                            onClick={() => void onTestShortener()}
                                        >
                                            Verbindung testen
                                        </Button>
                                    </div>
                                </SettingsSection>
                            </TabsContent>

                            <TabsContent value="whatsapp" className="mt-4 space-y-4">
                                <SettingsSection title="Twilio WhatsApp">
                                    <div className="space-y-3">
                                        <Field label="Account SID">
                                            <PasswordInput
                                                autoComplete="off"
                                                value={whatsapp.twilio_account_sid}
                                                onChange={(e) =>
                                                    setWhatsapp((p) => ({
                                                        ...p,
                                                        twilio_account_sid: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="Auth Token">
                                            <PasswordInput
                                                autoComplete="off"
                                                value={whatsapp.twilio_auth_token}
                                                onChange={(e) =>
                                                    setWhatsapp((p) => ({
                                                        ...p,
                                                        twilio_auth_token: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                        <Field label="WhatsApp From">
                                            <Input
                                                placeholder="whatsapp:+49…"
                                                value={whatsapp.twilio_whatsapp_from}
                                                onChange={(e) =>
                                                    setWhatsapp((p) => ({
                                                        ...p,
                                                        twilio_whatsapp_from: e.target.value,
                                                    }))
                                                }
                                            />
                                        </Field>
                                    </div>
                                </SettingsSection>
                            </TabsContent>

                            <TabsContent value="extras" className="mt-4 space-y-4">
                                <SettingsSection
                                    title="Einrichtung"
                                    description="First-Run-Assistent erneut öffnen oder Kernpfade zurücksetzen."
                                >
                                    <div className="flex flex-wrap gap-2">
                                        <Button
                                            type="button"
                                            variant="secondary"
                                            onClick={() => {
                                                void (async () => {
                                                    try {
                                                        await resetSetup(false);
                                                        onOpenSetupWizard?.();
                                                        onClose();
                                                    } catch (err) {
                                                        showError(String(err), "Einrichtung");
                                                    }
                                                })();
                                            }}
                                        >
                                            Einrichtungsassistent öffnen
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="secondary"
                                            onClick={() => {
                                                void (async () => {
                                                    const ok = await confirm(
                                                        "Pfade (Monitor/Archiv/Log) leeren und Assistent erneut starten?",
                                                        {
                                                            title: "Einrichtung zurücksetzen",
                                                            primaryLabel: "Zurücksetzen",
                                                            destructive: true,
                                                        },
                                                    );
                                                    if (!ok) return;
                                                    try {
                                                        await resetSetup(true);
                                                        onOpenSetupWizard?.();
                                                        onClose();
                                                    } catch (err) {
                                                        showError(String(err), "Einrichtung");
                                                    }
                                                })();
                                            }}
                                        >
                                            Factory-Reset (Pfade)
                                        </Button>
                                    </div>
                                </SettingsSection>

                                <SettingsSection
                                    title="Legacy-Migration"
                                    description="Einmaliger Import aus QSettings (AKSoftware/AeroMediaService) und Keyring (DropboxUploaderApp). Secrets nur in den v2-Keyring — nie in SQLite."
                                >
                                    <Button
                                        type="button"
                                        variant="secondary"
                                        onClick={() => {
                                            void (async () => {
                                                try {
                                                    const report = await migrateLegacySettings(true);
                                                    showSuccess(report.message, "Legacy-Migration");
                                                } catch (err) {
                                                    showError(String(err), "Legacy-Migration");
                                                }
                                            })();
                                        }}
                                    >
                                        Migration erneut ausführen
                                    </Button>
                                </SettingsSection>

                                <SettingsSection title="Software-Update">
                                    <div className="space-y-2">
                                        <p className="text-xs text-muted">
                                            Aktuell installierte Version:{" "}
                                            <span className="font-medium text-foreground">
                        {appVersion || "—"}
                      </span>
                                        </p>
                                        <p className="text-xs text-muted">
                                            Update-Status:{" "}
                                            <span className="font-medium text-foreground">
                        {updaterMessage}
                      </span>
                                        </p>
                                        {platformHint ? (
                                            <p className="text-xs text-muted">{platformHint}</p>
                                        ) : null}
                                        {installBlockedReason ? (
                                            <p className="text-xs text-destructive">
                                                {installBlockedReason}
                                            </p>
                                        ) : null}
                                        <Button
                                            type="button"
                                            variant="secondary"
                                            size="sm"
                                            disabled={Boolean(installBlockedReason)}
                                            onClick={() => onRequestUpdateCheck?.()}
                                        >
                                            Jetzt auf Updates prüfen
                                        </Button>
                                    </div>
                                </SettingsSection>

                                <SettingsSection
                                    title="Version wechseln"
                                    description="Verfügbare stabile Versionen (stille Installation wie Auto-Update)."
                                >
                                    <div className="space-y-3">
                                        <label className="flex items-center gap-2 text-sm">
                                            <Checkbox
                                                checked={showPrereleases}
                                                onCheckedChange={(v) => setShowPrereleases(v === true)}
                                            />
                                            Prereleases anzeigen
                                        </label>
                                        <Field label="Ziel-Version">
                                            <Select
                                                value={selectedVersion || undefined}
                                                onValueChange={setSelectedVersion}
                                                disabled={
                                                    releasesLoading || filteredReleases.length === 0
                                                }
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
                                                    {filteredReleases.map((r, index) => {
                                                        const labels: string[] = [];
                                                        if (index === 0) labels.push("Neueste");
                                                        if (
                                                            appVersion &&
                                                            compareVersionParts(r.tag_name, appVersion) === 0
                                                        ) {
                                                            labels.push("Installiert");
                                                        }
                                                        if (r.prerelease) labels.push("Prerelease");
                                                        if (!r.updater_json_url) {
                                                            labels.push("nicht auto-installierbar");
                                                        }
                                                        const suffix =
                                                            labels.length > 0
                                                                ? ` (${labels.join(", ")})`
                                                                : "";
                                                        return (
                                                            <SelectItem key={r.tag_name} value={r.tag_name}>
                                                                {r.tag_name}
                                                                {suffix}
                                                            </SelectItem>
                                                        );
                                                    })}
                                                </SelectContent>
                                            </Select>
                                        </Field>
                                        <div className="flex flex-wrap gap-2">
                                            <Button
                                                type="button"
                                                variant="secondary"
                                                size="sm"
                                                disabled={releasesLoading}
                                                onClick={() => void loadExtras()}
                                            >
                                                Liste neu laden
                                            </Button>
                                            <Button
                                                type="button"
                                                size="sm"
                                                disabled={
                                                    !selectedRelease ||
                                                    selectedRelation === "same" ||
                                                    Boolean(installBlockedReason) ||
                                                    (!selectedRelease.updater_json_url &&
                                                        !selectedRelease.installer_url)
                                                }
                                                onClick={() => {
                                                    if (!selectedRelease) return;
                                                    onRequestVersionSwitch?.(selectedRelease);
                                                }}
                                            >
                                                {!selectedRelease?.updater_json_url &&
                                                selectedRelease?.installer_url
                                                    ? "Installer öffnen…"
                                                    : selectedRelation === "older"
                                                        ? "Ältere Version installieren"
                                                        : selectedRelation === "newer"
                                                            ? "Aktualisieren"
                                                            : "Auf diese Version wechseln"}
                                            </Button>
                                        </div>
                                        {releasesError ? (
                                            <p className="text-xs text-destructive">{releasesError}</p>
                                        ) : null}
                                        {selectedRelease && selectedRelation !== "same" ? (
                                            <div
                                                className="space-y-1 rounded-md border border-border/50 bg-card/40 p-3">
                                                {!selectedRelease.updater_json_url ? (
                                                    <p className="text-xs text-muted">
                                                        Für diese Version ist die automatische Installation
                                                        nicht verfügbar.
                                                    </p>
                                                ) : null}
                                                <p className="text-sm font-medium">Release-Notes</p>
                                                <pre
                                                    className="max-h-28 overflow-y-auto whitespace-pre-wrap text-xs text-muted">
                          {selectedRelease.body || "Keine Details verfügbar."}
                        </pre>
                                            </div>
                                        ) : null}
                                    </div>
                                </SettingsSection>
                            </TabsContent>
                        </div>
                    </Tabs>

                    <div className="flex shrink-0 flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                        <div className="min-h-[1.25rem] text-sm">
                            {loadError ? (
                                <span className="text-destructive">{loadError}</span>
                            ) : isDirty ? (
                                <span className="text-muted">Ungespeicherte Änderungen</span>
                            ) : null}
                        </div>
                        <DialogFooter className="gap-2 sm:justify-end">
                            <Button
                                type="button"
                                variant="secondary"
                                onClick={() => void requestClose()}
                            >
                                Schließen
                            </Button>
                            <Button type="submit" disabled={busy}>
                                {busy ? "Speichern…" : "Speichern & Übernehmen"}
                            </Button>
                        </DialogFooter>
                    </div>
                </form>
            </DialogContent>
        </Dialog>
    );
}
