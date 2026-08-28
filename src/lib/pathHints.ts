/** Client-side mirror of `bridge::types::PathHintsStatus` (P6d). */

import {
  isNetworkShareUrl,
  sanitizeSharePath,
} from "@/lib/smbUrl";

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

export function toSmbUrl(raw: string): string {
  return sanitizeSharePath(raw);
}

export function isNetworkSharePath(raw: string): boolean {
  return isNetworkShareUrl(raw);
}

function normalizeSmbForCompare(raw: string): string {
  let s = sanitizeSharePath(raw).toLowerCase().replace(/\\/g, "/");
  while (s.endsWith("/") && s.length > 1) {
    s = s.slice(0, -1);
  }
  return s;
}

export function evaluatePathHints(
  bridgeEnabled: boolean,
  monitorPath: string,
  primaryRaw: string,
): PathHintsStatus {
  const primarySmbUrl = toSmbUrl(primaryRaw);
  const pathsV1 = primarySmbUrl.length > 0;
  const monitorIsNetworkShare = isNetworkSharePath(monitorPath);
  const monitorSmbUrl = monitorIsNetworkShare ? toSmbUrl(monitorPath) : "";
  const suggestedPrimarySmbUrl = monitorIsNetworkShare
    ? monitorSmbUrl
    : monitorPath.trim();

  let drift: PathHintsDrift;
  if (!bridgeEnabled) {
    drift = "disabled";
  } else if (!pathsV1) {
    drift = "missing_primary";
  } else if (monitorIsNetworkShare) {
    drift =
      normalizeSmbForCompare(primarySmbUrl) === normalizeSmbForCompare(monitorSmbUrl)
        ? "ok"
        : "drift";
  } else {
    drift = "ok";
  }

  let warning: string | null = null;
  if (drift === "missing_primary") {
    const monitorHint = monitorSmbUrl || monitorPath.trim();
    warning = monitorHint
      ? `Primär-Share fehlt — paths-v1 ist inaktiv. Monitor: ${monitorHint}`
      : "Primär-Share fehlt — Capability paths-v1 ist nicht aktiv.";
  } else if (drift === "drift") {
    warning = `Primär-Share weicht vom Monitor-Pfad ab (Primär: ${primarySmbUrl}, Monitor: ${monitorSmbUrl}).`;
  }

  return {
    bridge_enabled: bridgeEnabled,
    paths_v1: pathsV1,
    monitor_is_network_share: monitorIsNetworkShare,
    suggested_primary_smb_url: suggestedPrimarySmbUrl,
    primary_smb_url: primarySmbUrl,
    monitor_smb_url: monitorSmbUrl,
    drift,
    warning,
  };
}

export function formatPathsV1Status(pathsV1: boolean, bridgeEnabled: boolean): string {
  if (!bridgeEnabled) return "—";
  return pathsV1 ? "aktiv" : "inaktiv";
}
