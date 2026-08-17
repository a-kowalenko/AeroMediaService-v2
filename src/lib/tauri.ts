/** Typed Tauri invoke wrappers (config, secrets, logging, monitor). */

import { invoke } from "@tauri-apps/api/core";

export type LogMessage = {
  level: number;
  level_name: string;
  message: string;
};

export function getAppVersion(): Promise<string> {
  return invoke<string>("get_app_version");
}

export function getSetting(key: string, fallback?: string): Promise<string> {
  return invoke<string>("get_setting", { key, default: fallback ?? null });
}

export function saveSetting(key: string, value: string): Promise<void> {
  return invoke("save_setting", { key, value });
}

export function getSecret(key: string): Promise<string | null> {
  return invoke<string | null>("get_secret", { key });
}

export function saveSecret(key: string, value: string): Promise<void> {
  return invoke("save_secret", { key, value });
}

export type MigrateReport = {
  skipped: boolean;
  settings_imported: number;
  secrets_imported: number;
  message: string;
};

export function migrateLegacySettings(force = false): Promise<MigrateReport> {
  return invoke<MigrateReport>("migrate_legacy_settings", { force });
}

export function resetSetup(clearPaths = false): Promise<void> {
  return invoke("reset_setup", { clear_paths: clearPaths });
}

export function getRecentLogs(limit?: number): Promise<LogMessage[]> {
  return invoke<LogMessage[]>("get_recent_logs", { limit: limit ?? null });
}

export function getMonitoringStatus(): Promise<boolean> {
  return invoke<boolean>("get_monitoring_status");
}

export type StabilityPendingItem = {
  dir_name: string;
  remaining_seconds: number;
  required_seconds: number;
  waiting_for_media: boolean;
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
  last_error: string | null;
};

export function getBridgeStatus(): Promise<BridgeStatus> {
  return invoke<BridgeStatus>("get_bridge_status");
}

export function applyBridgeConfig(): Promise<BridgeStatus> {
  return invoke<BridgeStatus>("apply_bridge_config");
}

export type ByteProgress = {
  percent: number;
  current: number;
  total: number;
};

export type QueueSnapshotItem = {
  position: number;
  dir_name: string;
  customer_label: string;
  state: string;
  wait_seconds: number;
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
  return invoke<HistoryEntry | null>("get_history_entry", { id });
}

export function deleteHistoryItems(ids: string[]): Promise<number> {
  return invoke<number>("delete_history_items", { ids });
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
  return invoke<string>("retry_upload", { id });
}

export function getSandboxWarnings(): Promise<string[]> {
  return invoke<string[]>("get_sandbox_warnings");
}

export function lookupShareLink(id: string): Promise<string> {
  return invoke<string>("lookup_share_link", { id });
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
  return invoke<HistoryEntry>("save_history_contact", { id, email, phone });
}

export function getManualStatusWarnings(id: string, action: string): Promise<string[]> {
  return invoke<string[]>("get_manual_status_warnings", { id, action });
}

export function setManualStatus(
  id: string,
  action: string,
  reason?: string,
): Promise<HistoryEntry> {
  return invoke<HistoryEntry>("set_manual_status", { id, action, reason: reason ?? "" });
}

export function channelsDelivered(
  id: string,
  sendEmail: boolean,
  sendSms: boolean,
): Promise<string[]> {
  return invoke<string[]>("channels_delivered", { id, sendEmail, sendSms });
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

export function verifyDropboxStatus(which: "native" | "custom"): Promise<string> {
  return invoke<string>("verify_dropbox_status", { which });
}

export function startDropboxOauth(which: "native" | "custom"): Promise<OauthStart> {
  return invoke<OauthStart>("start_dropbox_oauth", { which });
}

export function finishDropboxOauth(
  which: "native" | "custom",
  authCode: string,
  codeVerifier: string,
): Promise<ConnectResult> {
  return invoke<ConnectResult>("finish_dropbox_oauth", {
    which,
    authCode,
    codeVerifier,
  });
}

export function connectDropbox(which: "native" | "custom"): Promise<ConnectResult> {
  return invoke<ConnectResult>("connect_dropbox", { which });
}

export function disconnectDropbox(which: "native" | "custom"): Promise<ConnectResult> {
  return invoke<ConnectResult>("disconnect_dropbox", { which });
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

export function checkForUpdates(): Promise<UpdateCheckResult> {
  return invoke<UpdateCheckResult>("check_for_updates");
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
  return invoke<string>("install_specific_version", { updaterJsonUrl });
}

/* ── Customer intake (Fertig-App replacement) ─────────────────────── */

export type Customer = {
  id: string;
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
  processed: boolean;
  assigned_path: string;
  created_at: string;
  updated_at: string;
};

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
};

export type MediaDirectoryListing = {
  path: string;
  parent: string;
  folders: MediaFolderInfo[];
};

export type AssignResult = {
  file_path: string;
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

export function saveCustomer(
  vorname: string,
  nachname: string,
  email: string,
  telefon?: string,
): Promise<Customer> {
  return invoke<Customer>("save_customer", {
    vorname,
    nachname,
    email,
    telefon: telefon ?? "",
  });
}

export function updateCustomer(customer: Customer): Promise<Customer> {
  return invoke<Customer>("update_customer", { customer });
}

export function deleteCustomer(id: string): Promise<void> {
  return invoke("delete_customer", { id });
}

export function setCustomerProcessed(
  id: string,
  processed: boolean,
): Promise<Customer> {
  return invoke<Customer>("set_customer_processed", { id, processed });
}

export function listMediaFolders(path?: string | null): Promise<MediaDirectoryListing> {
  return invoke<MediaDirectoryListing>("list_media_folders_cmd", {
    path: path ?? null,
  });
}

export function assignCustomerToFolder(
  id: string,
  targetPath: string,
): Promise<AssignResult> {
  return invoke<AssignResult>("assign_customer_to_folder", {
    id,
    targetPath,
  });
}

export function getAssignmentHistory(): Promise<AssignmentHistoryEntry[]> {
  return invoke<AssignmentHistoryEntry[]>("get_assignment_history");
}
