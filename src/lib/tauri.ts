/** Typed Tauri invoke wrappers (config, secrets, logging, monitor). */

import {invoke} from "@tauri-apps/api/core";

export type LogMessage = {
    level: number;
    level_name: string;
    message: string;
};

export function getAppVersion(): Promise<string> {
    return invoke<string>("get_app_version");
}

export function openExternalUrl(url: string): Promise<void> {
    return invoke("open_external_url", {url});
}

export function openExternalPath(path: string): Promise<void> {
    return invoke("open_external_path", {path});
}

export function getSetting(key: string, fallback?: string): Promise<string> {
    return invoke<string>("get_setting", {key, default: fallback ?? null});
}

export function saveSetting(key: string, value: string): Promise<void> {
    return invoke("save_setting", {key, value});
}

export function getSecret(key: string): Promise<string | null> {
    return invoke<string | null>("get_secret", {key});
}

export function saveSecret(key: string, value: string): Promise<void> {
    return invoke("save_secret", {key, value});
}

export type BrochureSourceInfo = {
    present: boolean;
    path: string;
    size_bytes: number;
    display_name: string;
};

export type BrochureStatus = {
    source: BrochureSourceInfo;
};

export function getBrochureStatus(): Promise<BrochureStatus> {
    return invoke<BrochureStatus>("get_brochure_status");
}

export function importBrochure(path: string): Promise<BrochureStatus> {
    return invoke<BrochureStatus>("import_brochure", {path});
}

export function removeBrochure(): Promise<BrochureStatus> {
    return invoke<BrochureStatus>("remove_brochure");
}

export function openBrochure(): Promise<void> {
    return invoke("open_brochure");
}

export type MigrateReport = {
    skipped: boolean;
    settings_imported: number;
    secrets_imported: number;
    message: string;
};

export function migrateLegacySettings(force = false): Promise<MigrateReport> {
    return invoke<MigrateReport>("migrate_legacy_settings", {force});
}

export function resetSetup(clearPaths = false): Promise<void> {
    return invoke("reset_setup", {clear_paths: clearPaths});
}

export type DefaultDirKind = "archive" | "logs";

export type DefaultDirsProposal = {
    root: string;
    archive_path: string;
    log_path: string;
    root_exists: boolean;
    archive_exists: boolean;
    log_exists: boolean;
    warnings: string[];
};

export type EnsureDefaultDirResult = {
    kind: DefaultDirKind;
    root: string;
    path: string;
    created: boolean;
    warnings: string[];
};

export type EnsureDefaultAppRootResult = {
    root: string;
    archive_path: string;
    log_path: string;
    created: boolean;
    warnings: string[];
};

export function proposeDefaultDirs(): Promise<DefaultDirsProposal> {
    return invoke<DefaultDirsProposal>("propose_default_dirs");
}

export function ensureDefaultDir(
    kind: DefaultDirKind,
    root?: string | null,
): Promise<EnsureDefaultDirResult> {
    return invoke<EnsureDefaultDirResult>("ensure_default_dir", {
        kind,
        root: root?.trim() ? root : null,
    });
}

export function ensureDefaultAppRoot(
    root?: string | null,
): Promise<EnsureDefaultAppRootResult> {
    return invoke<EnsureDefaultAppRootResult>("ensure_default_app_root", {
        root: root?.trim() ? root : null,
    });
}

export function getRecentLogs(limit?: number): Promise<LogMessage[]> {
    return invoke<LogMessage[]>("get_recent_logs", {limit: limit ?? null});
}

export function getMonitoringStatus(): Promise<boolean> {
    return invoke<boolean>("get_monitoring_status");
}

export type StabilityPendingItem = {
    dir_name: string;
    remaining_seconds: number;
    required_seconds: number;
    waiting_for_media: boolean;
    kind?: string;
    correlation_id?: string;
    handoff_phase?: string;
    handoff_error_code?: string;
    handoff_error_message?: string;
};

export function getStabilityPending(): Promise<StabilityPendingItem[]> {
    return invoke<StabilityPendingItem[]>("get_stability_pending");
}

export function startMonitoring(): Promise<boolean> {
    return invoke<boolean>("start_monitoring");
}

export function stopMonitoring(): Promise<void> {
    return invoke("stop_monitoring");
}

export type BridgeStatus = {
    running: boolean;
    bind_addr: string;
    display_name: string;
    instance_id: string;
    mdns_active: boolean;
    last_error: string | null;
};

export function getBridgeStatus(): Promise<BridgeStatus> {
    return invoke<BridgeStatus>("get_bridge_status");
}

export function applyBridgeConfig(): Promise<BridgeStatus> {
    return invoke<BridgeStatus>("apply_bridge_config");
}

export type PathHintsDrift = "disabled" | "ok" | "missing_primary" | "drift";

export type PathHintsStatus = {
    bridge_enabled: boolean;
    paths_v1: boolean;
    monitor_is_network_share: boolean;
    suggested_primary_smb_url: string;
    primary_smb_url: string;
    monitor_smb_url: string;
    drift: PathHintsDrift;
    warning: string | null;
};

export function getPathHintsStatus(): Promise<PathHintsStatus> {
    return invoke<PathHintsStatus>("get_path_hints_status");
}

export type AtsPresenceCategory = "connected" | "disconnected" | "inactive_long";

export type AtsHostSummary = {
    instance_id: string;
    hostname: string;
    display_label: string;
    ats_version: string;
    ats_app: string;
    last_event_type: string;
    last_event_at: string;
    last_seen_at: string;
    is_connected: boolean;
    is_active: boolean;
    presence_category: AtsPresenceCategory;
    activity_count_ttl: number;
    jobs_count_ttl: number;
    degraded_identity: boolean;
};

export type AtsActivityEntry = {
    occurred_at: string;
    event_type: string;
    route: string;
    method: string;
    status_code_class: string;
    correlation_id: string;
    folder_name: string;
    payload_json: string;
};

export type AtsJobOriginView = {
    correlation_id: string;
    folder_name: string;
    first_seen_at: string;
    last_seen_at: string;
    source_event_type: string;
    ams_status_label: string;
};

export type AtsHostDetails = {
    host: {
        instance_id: string;
        hostname: string;
        display_label: string;
        ats_version: string;
        ats_app: string;
        first_seen_at: string;
        last_seen_at: string;
        last_event_type: string;
        last_event_at: string;
        is_connected: boolean;
        is_active: boolean;
        presence_category: AtsPresenceCategory;
        degraded_identity: boolean;
        activity_window_minutes: number;
        recent_events: AtsActivityEntry[];
    };
    recent_jobs: AtsJobOriginView[];
};

export type AtsJobsPage = {
    items: AtsJobOriginView[];
    total: number;
    page: number;
    page_size: number;
};

export function getAtsHostsSummary(ttlMinutes = 60): Promise<AtsHostSummary[]> {
    return invoke<AtsHostSummary[]>("get_ats_hosts_summary", {ttlMinutes});
}

export function getAtsHostDetails(
    instanceId: string,
    ttlMinutes = 60,
    limit = 100,
): Promise<AtsHostDetails | null> {
    return invoke<AtsHostDetails | null>("get_ats_host_details", {
        instanceId,
        ttlMinutes,
        limit,
    });
}

export function getAtsJobsByHost(
    instanceId: string,
    ttlMinutes = 60,
    page = 0,
    pageSize = 50,
): Promise<AtsJobsPage> {
    return invoke<AtsJobsPage>("get_ats_jobs_by_host", {
        instanceId,
        ttlMinutes,
        page,
        pageSize,
    });
}

export type AtsActivityPage = {
    items: AtsActivityEntry[];
    total: number;
    offset: number;
    limit: number;
    has_more: boolean;
};

export function getAtsHostActivity(
    instanceId: string,
    offset = 0,
    limit = 10,
): Promise<AtsActivityPage> {
    return invoke<AtsActivityPage>("get_ats_host_activity", {
        instanceId,
        offset,
        limit,
    });
}

export function removeAtsHost(instanceId: string): Promise<boolean> {
    return invoke<boolean>("remove_ats_host", {instanceId});
}

export function removeInactiveLongAtsHosts(): Promise<number> {
    return invoke<number>("remove_inactive_long_ats_hosts");
}

export type ByteProgress = {
    percent: number;
    current: number;
    total: number;
};

/** One in-flight upload on a worker lane. */
export type UploadActiveSlot = {
    /** 0-based worker lane (top row = 0). */
    worker_index: number;
    /** 1-based file index within the job. */
    file_index: number;
    name: string;
    percent: number;
    current: number;
    total: number;
};

/** Completed count + currently active parallel slots. */
export type UploadSlotsProgress = {
    files_done: number;
    files_total: number;
    slots: UploadActiveSlot[];
};

export type UploadActivityPhase =
    | "idle"
    | "starting"
    | "uploading"
    | "finalizing"
    | "registering"
    | "linking"
    | "paused"
    | "pausing"
    | "appending"
    | "success"
    | "failed"
    | "cancelled";

/** Structured upload activity — prefer over free-form status strings in the panel. */
export type UploadActivity = {
    phase: UploadActivityPhase;
    dir_name?: string;
    rel_path?: string;
    /** 1-based file index while uploading. */
    file_index?: number;
    file_count?: number;
    /** Short path-free phrase or error summary. */
    message?: string;
};

export type QueueSnapshotItem = {
    position: number;
    dir_name: string;
    customer_label: string;
    state: string;
    wait_seconds: number;
};

export type UploadControlState = {
    paused: boolean;
    holding: boolean;
    cancelled: boolean;
};

export function pauseUpload(): Promise<void> {
    return invoke("pause_upload");
}

export function resumeUpload(): Promise<void> {
    return invoke("resume_upload");
}

export function cancelUpload(): Promise<void> {
    return invoke("cancel_upload");
}

export function getUploadQueue(): Promise<QueueSnapshotItem[]> {
    return invoke<QueueSnapshotItem[]>("get_upload_queue");
}

export function getUploadControlState(): Promise<UploadControlState> {
    return invoke<UploadControlState>("get_upload_control_state");
}

export type HistoryEntry = {
    id: string;
    dir_name: string;
    status: string;
    email_status: string;
    sms_status: string;
    error_msg: string;
    first_name: string;
    last_name: string;
    email: string;
    phone: string;
    customer_number: string;
    booking_number: string;
    "type"?: string;
    marker_raw: string;
    remote_path: string;
    share_link: string;
    sms_id: string;
    archived_path: string;
    archive_subfolder: string;
    last_sms_resent_at: string;
    sms_status_locked: boolean;
    created_at: string;
    last_updated: string;
    overall_status: string;
    combined_error: string;
    display_name: string;
    extra?: Record<string, unknown>;
};

export type HistoryAppendEvent = {
    kind?: string;
    source_dir_name?: string;
    parent_dir_name?: string;
    state?: string;
    created_at?: string;
    updated_at?: string;
    completed_at?: string;
    archived_path?: string;
    error_msg?: string;
    share_link?: string;
    order_id?: string;
    correlation_id?: string;
    remote_path?: string;
    marker_raw?: string;
};

export type HistoryPage = {
    items: HistoryEntry[];
    total: number;
    page: number;
    page_size: number;
};

export function getHistory(
    search?: string,
    page?: number,
    pageSize?: number,
): Promise<HistoryPage> {
    return invoke<HistoryPage>("get_history", {
        search: search ?? "",
        page: page ?? 0,
        pageSize: pageSize ?? 25,
    });
}

export function getHistoryEntry(id: string): Promise<HistoryEntry | null> {
    return invoke<HistoryEntry | null>("get_history_entry", {id});
}

export function deleteHistoryItems(ids: string[]): Promise<number> {
    return invoke<number>("delete_history_items", {ids});
}

export function clearHistory(): Promise<void> {
    return invoke("clear_history");
}

export type ResendCommandResult = {
    message: string;
    had_failures: boolean;
    email_status: string | null;
    sms_status: string | null;
};

export function retryUpload(id: string): Promise<string> {
    return invoke<string>("retry_upload", {id});
}

export function appendHistoryMedia(id: string, localDir: string): Promise<string> {
    return invoke<string>("append_history_media", {id, localDir});
}

export type AppendCategoryId =
    | "handcam_video"
    | "handcam_foto"
    | "outside_video"
    | "outside_foto";

export type AppendFileItem = {
    path: string;
    category: AppendCategoryId;
    preview: boolean;
};

export function appendHistoryFiles(
    id: string,
    items: AppendFileItem[],
): Promise<string> {
    return invoke<string>("append_history_files", {id, items});
}

export type HistoryBookingFlags = {
    handcam_foto: boolean;
    handcam_video: boolean;
    outside_foto: boolean;
    outside_video: boolean;
    ist_bezahlt_handcam_foto: boolean;
    ist_bezahlt_handcam_video: boolean;
    ist_bezahlt_outside_foto: boolean;
    ist_bezahlt_outside_video: boolean;
};

export type BookingFlagsPolicy = "cache" | "auto" | "force";

export type HistoryBookingFlagsResult = HistoryBookingFlags & {
    lookup: "cache" | "api" | "skipped" | string;
    updated_at: string | null;
    can_refresh: boolean;
};

export function resolveHistoryBookingFlags(
    id: string,
    policy: BookingFlagsPolicy = "auto",
): Promise<HistoryBookingFlagsResult> {
    return invoke<HistoryBookingFlagsResult>("resolve_history_booking_flags", {
        id,
        policy,
    });
}

export function expandAppendMediaPaths(paths: string[]): Promise<string[]> {
    return invoke<string[]>("expand_append_media_paths", {paths});
}

export function getSandboxWarnings(): Promise<string[]> {
    return invoke<string[]>("get_sandbox_warnings");
}

export function lookupShareLink(id: string): Promise<string> {
    return invoke<string>("lookup_share_link", {id});
}

export function resendHistoryNotifications(
    id: string,
    email: string,
    phone: string,
    shareLink: string,
    sendEmail: boolean,
    sendSms: boolean,
): Promise<ResendCommandResult> {
    return invoke<ResendCommandResult>("resend_history_notifications", {
        id,
        email,
        phone,
        shareLink,
        sendEmail,
        sendSms,
    });
}

export function saveHistoryContact(
    id: string,
    email: string,
    phone: string,
): Promise<HistoryEntry> {
    return invoke<HistoryEntry>("save_history_contact", {id, email, phone});
}

export function getManualStatusWarnings(id: string, action: string): Promise<string[]> {
    return invoke<string[]>("get_manual_status_warnings", {id, action});
}

export function setManualStatus(
    id: string,
    action: string,
    reason?: string,
): Promise<HistoryEntry> {
    return invoke<HistoryEntry>("set_manual_status", {id, action, reason: reason ?? ""});
}

export function channelsDelivered(
    id: string,
    sendEmail: boolean,
    sendSms: boolean,
): Promise<string[]> {
    return invoke<string[]>("channels_delivered", {id, sendEmail, sendSms});
}

export function syncSmsJournal(): Promise<number> {
    return invoke<number>("sync_sms_journal");
}

export type ConnectResult = {
    success: boolean;
    status: string;
    message: string;
    needs_oauth: boolean;
    authorize_url: string | null;
    code_verifier: string | null;
};

export type OauthStart = {
    authorize_url: string;
    code_verifier: string;
};

export function getCloudConnectionStatus(): Promise<string> {
    return invoke<string>("get_cloud_connection_status");
}

export type DropboxAccountInfo = {
    account_id: string;
    display_name: string;
    email: string;
    profile_photo_url: string;
    app_name: string;
    app_key_hint: string;
    token_valid: boolean;
    used_bytes: number;
    allocated_bytes: number | null;
};

export type DropboxAccountPool = "native" | "custom_api";

/** IPC `which` for connect/verify/oauth (`custom` aliases `custom_api`). */
export function dropboxPoolWhich(
    pool: DropboxAccountPool,
): "native" | "custom" {
    return pool === "native" ? "native" : "custom";
}

export function dropboxAccountSecretKeys(
    pool: DropboxAccountPool,
    amsId: string,
): { app_key: string; app_secret: string } {
    const id = amsId.trim();
    if (pool === "native") {
        return {
            app_key: `db_app_key_${id}`,
            app_secret: `db_app_secret_${id}`,
        };
    }
    return {
        app_key: `custom_db_app_key_${id}`,
        app_secret: `custom_db_app_secret_${id}`,
    };
}

export function dropboxActiveSettingKey(pool: DropboxAccountPool): string {
    return pool === "native"
        ? "active_dropbox_account_id"
        : "active_custom_dropbox_account_id";
}

export type DropboxAccountRow = {
    id: string;
    pool: string;
    label: string;
    dropbox_account_id: string;
    email: string;
    display_name: string;
    app_key_hint: string;
    app_folder_name: string;
    created_at: string;
    updated_at: string;
};

export function getDropboxAccountInfo(
    which: "native" | "custom",
    accountId?: string | null,
): Promise<DropboxAccountInfo> {
    return invoke<DropboxAccountInfo>("get_dropbox_account_info", {
        which,
        accountId: accountId ?? null,
    });
}

export function listDropboxAccounts(
    pool: DropboxAccountPool,
): Promise<DropboxAccountRow[]> {
    return invoke<DropboxAccountRow[]>("list_dropbox_accounts", {pool});
}

export function createDropboxAccount(
    pool: DropboxAccountPool,
    label?: string | null,
    appFolderName?: string | null,
): Promise<DropboxAccountRow> {
    return invoke<DropboxAccountRow>("create_dropbox_account", {
        pool,
        label: label ?? null,
        appFolderName: appFolderName ?? null,
    });
}

export function setActiveDropboxAccount(
    pool: DropboxAccountPool,
    accountId: string,
): Promise<DropboxAccountRow> {
    return invoke<DropboxAccountRow>("set_active_dropbox_account", {
        pool,
        accountId,
    });
}

export function renameDropboxAccount(
    accountId: string,
    label: string,
    appFolderName?: string | null,
): Promise<DropboxAccountRow> {
    return invoke<DropboxAccountRow>("rename_dropbox_account", {
        accountId,
        label,
        appFolderName: appFolderName ?? null,
    });
}

export function setDropboxAppFolderName(
    accountId: string,
    appFolderName: string,
): Promise<DropboxAccountRow> {
    return invoke<DropboxAccountRow>("set_dropbox_app_folder_name", {
        accountId,
        appFolderName,
    });
}

export function deleteDropboxAccount(accountId: string): Promise<void> {
    return invoke("delete_dropbox_account", {accountId});
}

export function verifyDropboxStatus(
    which: "native" | "custom",
    accountId?: string | null,
): Promise<string> {
    return invoke<string>("verify_dropbox_status", {
        which,
        accountId: accountId ?? null,
    });
}

export function startDropboxOauth(
    which: "native" | "custom",
    accountId?: string | null,
): Promise<OauthStart> {
    return invoke<OauthStart>("start_dropbox_oauth", {
        which,
        accountId: accountId ?? null,
    });
}

export function finishDropboxOauth(
    which: "native" | "custom",
    authCode: string,
    codeVerifier: string,
    accountId?: string | null,
): Promise<ConnectResult> {
    return invoke<ConnectResult>("finish_dropbox_oauth", {
        which,
        authCode,
        codeVerifier,
        accountId: accountId ?? null,
    });
}

export function connectDropbox(
    which: "native" | "custom",
    accountId?: string | null,
): Promise<ConnectResult> {
    return invoke<ConnectResult>("connect_dropbox", {
        which,
        accountId: accountId ?? null,
    });
}

export function disconnectDropbox(
    which: "native" | "custom",
    accountId?: string | null,
): Promise<ConnectResult> {
    return invoke<ConnectResult>("disconnect_dropbox", {
        which,
        accountId: accountId ?? null,
    });
}

export function connectCustomApi(): Promise<ConnectResult> {
    return invoke<ConnectResult>("connect_custom_api");
}

export function disconnectCustomApi(): Promise<ConnectResult> {
    return invoke<ConnectResult>("disconnect_custom_api");
}

export function connectActiveCloud(): Promise<ConnectResult> {
    return invoke<ConnectResult>("connect_active_cloud");
}

export function disconnectActiveCloud(): Promise<ConnectResult> {
    return invoke<ConnectResult>("disconnect_active_cloud");
}

export function autoConnectCloud(): Promise<ConnectResult> {
    return invoke<ConnectResult>("auto_connect_cloud");
}

export function getSmsBalance(apiKey?: string, sandbox?: boolean): Promise<string> {
    return invoke<string>("get_sms_balance", {
        apiKey: apiKey ?? null,
        sandbox: sandbox ?? null,
    });
}

export function testLinkShortener(
    baseUrl: string,
    apiKey: string,
    expiresPreset?: string,
): Promise<string> {
    return invoke<string>("test_link_shortener", {
        baseUrl,
        apiKey,
        expiresPreset: expiresPreset ?? null,
    });
}

export type UpdaterStatus = {
    configured: boolean;
    current_version: string;
    message: string;
};

export type UpdateCheckResult = {
    configured: boolean;
    available: boolean;
    current_version: string;
    latest_version: string | null;
    body: string | null;
    message: string;
    prerelease: boolean;
    updater_json_url: string | null;
    installer_url: string | null;
};

export type UpdateInstallProgress = {
    phase: "download" | "install" | string;
    downloadedBytes: number;
    totalBytes: number | null;
    percent: number;
    speedBps: number;
};

export type AvailableRelease = {
    tag_name: string;
    published_at: string;
    body: string;
    installer_url: string | null;
    updater_json_url: string | null;
    prerelease: boolean;
};

export function getUpdaterStatus(): Promise<UpdaterStatus> {
    return invoke<UpdaterStatus>("get_updater_status");
}

export function getUpdaterInstallHint(): Promise<string | null> {
    return invoke<string | null>("get_updater_install_hint");
}

export function checkForUpdates(includeBeta = false): Promise<UpdateCheckResult> {
    return invoke<UpdateCheckResult>("check_for_updates", { includeBeta });
}

export function installUpdate(): Promise<string> {
    return invoke<string>("install_update");
}

export function cancelUpdateInstall(): Promise<boolean> {
    return invoke<boolean>("cancel_update_install");
}

export function listAvailableVersions(): Promise<AvailableRelease[]> {
    return invoke<AvailableRelease[]>("list_available_versions");
}

export function installSpecificVersion(updaterJsonUrl: string): Promise<string> {
    return invoke<string>("install_specific_version", {updaterJsonUrl});
}

/* ── Customer intake (Fertig-App replacement) ─────────────────────── */

export type Customer = {
    id: string;
    vorname: string;
    nachname: string;
    email: string;
    telefon: string;
    kunden_id?: string;
    booking_id?: string;
    booking_date?: string;
    typ?: string;
    handcam_foto?: boolean;
    handcam_video?: boolean;
    outside_foto?: boolean;
    outside_video?: boolean;
    ist_bezahlt_handcam_foto?: boolean;
    ist_bezahlt_handcam_video?: boolean;
    ist_bezahlt_outside_foto?: boolean;
    ist_bezahlt_outside_video?: boolean;
    media_option?: string;
    processed: boolean;
    assigned_path: string;
    created_at: string;
    updated_at: string;
};

export type CustomerDraft = {
    vorname: string;
    nachname: string;
    email: string;
    telefon?: string;
    kunden_id?: string;
    booking_id?: string;
    booking_date?: string;
    typ?: string;
    handcam_foto?: boolean;
    handcam_video?: boolean;
    outside_foto?: boolean;
    outside_video?: boolean;
    ist_bezahlt_handcam_foto?: boolean;
    ist_bezahlt_handcam_video?: boolean;
    ist_bezahlt_outside_foto?: boolean;
    ist_bezahlt_outside_video?: boolean;
    media_option?: string;
};

export type IntakeLookupHit = {
    vorname: string;
    nachname: string;
    email: string;
    telefon: string;
    kunden_id: string;
    booking_id: string;
    booking_date: string;
    typ: string;
    handcam_foto: boolean;
    handcam_video: boolean;
    outside_foto: boolean;
    outside_video: boolean;
    ist_bezahlt_handcam_foto: boolean;
    ist_bezahlt_handcam_video: boolean;
    ist_bezahlt_outside_foto: boolean;
    ist_bezahlt_outside_video: boolean;
    media_option: string;
};

export type IntakeLookupResult =
    | { kind: "hit"; customer: IntakeLookupHit }
    | { kind: "choice"; handcam: IntakeLookupHit; outside: IntakeLookupHit }
    | { kind: "not_found" }
    | { kind: "error"; message: string };

export type AssignmentHistoryEntry = {
    id: string;
    customer_id: string;
    vorname: string;
    nachname: string;
    email: string;
    telefon: string;
    file_path: string;
    created_at: string;
};

export type FolderState = "ready" | "busy" | "occupied";

export type MediaFolderInfo = {
    name: string;
    path: string;
    is_ready: boolean;
    block_reason: string | null;
    folder_state: FolderState;
    match_score?: number;
    recommended?: boolean;
};

export type MediaDirectoryListing = {
    path: string;
    parent: string;
    folders: MediaFolderInfo[];
};

export type AssignResult = {
    file_path: string;
    folder_path?: string;
};

export type IdAssignOverride = {
    tandemmaster?: string | null;
    videospringer?: string | null;
    dropzone_suffix?: string | null;
};

export type IdAssignPreview = {
    customer_id: string;
    customer_label: string;
    folder_path: string;
    folder_name: string;
    preview_folder_name: string;
    needs_review: boolean;
    review_reasons: string[];
    tandemmaster: string | null;
    videospringer: string | null;
    dropzone_suffix: string | null;
    tm_confidence: number;
    vs_confidence: number;
    outside_video: boolean;
    vs_required: boolean;
    can_confirm: boolean;
    booking_date: string;
    crew: Array<{
        name: string;
        tandemmaster: boolean;
        videospringer: boolean;
        aliases: string[];
    }>;
    /** Tokens skipped as guest (Phase 19e). */
    skipped_guest_tokens?: string[];
    /** Crew taken from post-TA/TD zone. */
    structured_crew_zone?: boolean;
};

export type BatchCustomerProposal = {
    customer: Customer;
    suggested_path: string | null;
    suggested_name: string | null;
    match_score: number;
    included: boolean;
};

export type BatchAssignmentProposal = {
    rows: BatchCustomerProposal[];
    folders: MediaFolderInfo[];
};

export type BatchAssignItem = {
    id: string;
    path: string;
    id_override?: IdAssignOverride | null;
};

export type BatchAssignOutcome = {
    assigned: Array<{ id: string; file_path: string }>;
    errors: Array<{ id: string; message: string }>;
};

export function listCustomers(
    search?: string,
    filter?: "all" | "unprocessed" | "processed",
): Promise<Customer[]> {
    return invoke<Customer[]>("list_customers", {
        search: search ?? "",
        filter: filter ?? "all",
    });
}

export function saveCustomer(draft: CustomerDraft): Promise<Customer> {
    return invoke<Customer>("save_customer", { draft });
}

export function updateCustomer(customer: Customer): Promise<Customer> {
    return invoke<Customer>("update_customer", { customer });
}

export function lookupCustomerIntake(
    kundenId: string,
    bookingId: string,
): Promise<IntakeLookupResult> {
    return invoke<IntakeLookupResult>("lookup_customer_intake", {
        kundenId,
        bookingId,
    });
}

export function deleteCustomer(id: string): Promise<void> {
    return invoke("delete_customer", { id });
}

export function setCustomerProcessed(
    id: string,
    processed: boolean,
): Promise<Customer> {
    return invoke<Customer>("set_customer_processed", {id, processed});
}

export function listMediaFolders(
    path?: string | null,
    vorname?: string | null,
    nachname?: string | null,
): Promise<MediaDirectoryListing> {
    return invoke<MediaDirectoryListing>("list_media_folders_cmd", {
        path: path ?? null,
        vorname: vorname ?? null,
        nachname: nachname ?? null,
    });
}

export function assignCustomerToFolder(
    id: string,
    targetPath: string,
    idOverride?: IdAssignOverride | null,
): Promise<AssignResult> {
    return invoke<AssignResult>("assign_customer_to_folder", {
        id,
        targetPath,
        idOverride: idOverride ?? null,
    });
}

export function previewIdAssign(
    id: string,
    targetPath: string,
    idOverride?: IdAssignOverride | null,
): Promise<IdAssignPreview> {
    return invoke<IdAssignPreview>("preview_id_assign", {
        id,
        targetPath,
        idOverride: idOverride ?? null,
    });
}

export function proposeCustomerAssignments(): Promise<BatchAssignmentProposal> {
    return invoke<BatchAssignmentProposal>("propose_customer_assignments");
}

export function assignCustomersBatch(
    items: BatchAssignItem[],
): Promise<BatchAssignOutcome> {
    return invoke<BatchAssignOutcome>("assign_customers_batch", {items});
}

export function getAssignmentHistory(): Promise<AssignmentHistoryEntry[]> {
    return invoke<AssignmentHistoryEntry[]>("get_assignment_history");
}
