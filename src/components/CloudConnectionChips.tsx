import { useCallback, useEffect, useState, type ComponentType } from "react";
import { Cloud, CloudOff, Server, ServerOff } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  dropboxActiveSettingKey,
  dropboxPoolWhich,
  getCloudConnectionStatus,
  getSetting,
  listDropboxAccounts,
  verifyDropboxStatus,
  type DropboxAccountPool,
  type DropboxAccountRow,
} from "@/lib/tauri";
import { isCloudConnected, useAppStore } from "@/store/appStore";

type Props = {
  /** Bump / flip to force a refresh (e.g. after Settings close). */
  refreshToken?: number;
  className?: string;
};

type ChipTone = "success" | "warning" | "muted" | "danger";

type StatusChipProps = {
  Icon: ComponentType<{ className?: string; "aria-hidden"?: boolean }>;
  label: string;
  title: string;
  tone: ChipTone;
};

function dropboxAccountTitle(row: DropboxAccountRow): string {
  const label = row.label.trim();
  if (label) return label;
  const name = row.display_name.trim();
  if (name) return name;
  const email = row.email.trim();
  if (email) return email;
  return "Dropbox";
}

function toneForStatus(status: string): ChipTone {
  const s = status.trim();
  if (isCloudConnected(s)) return "success";
  if (!s || s === "Nicht verbunden" || s.startsWith("Nicht verbunden")) {
    return "muted";
  }
  if (s.includes("Fehler") || s.includes("fehler") || s.includes("Konflikt")) {
    return "danger";
  }
  return "warning";
}

function StatusChip({ Icon, label, title, tone }: StatusChipProps) {
  return (
    <div
      className={cn(
        "inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] leading-none shadow-sm",
        tone === "success" &&
          "border-success/30 bg-success/10 text-success",
        tone === "warning" &&
          "border-warning/30 bg-warning/10 text-warning",
        tone === "danger" &&
          "border-destructive/30 bg-destructive/10 text-destructive",
        tone === "muted" &&
          "border-border bg-card/80 text-muted",
      )}
      title={title}
    >
      <Icon className="h-3 w-3 shrink-0" aria-hidden />
      <span className="truncate">{label}</span>
    </div>
  );
}

/** Sidebar footer: Custom-API + Dropbox connection chips (or Dropbox alone). */
export function CloudConnectionChips({ refreshToken = 0, className }: Props) {
  const connectionStatus = useAppStore((s) => s.connectionStatus);
  const [cloudService, setCloudService] = useState<"dropbox" | "custom_api">(
    "dropbox",
  );
  const [apiStatus, setApiStatus] = useState("");
  const [dropboxStatus, setDropboxStatus] = useState("");
  const [dropboxLabel, setDropboxLabel] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const selected = (await getSetting("selected_cloud_service", "dropbox"))
      .trim()
      .toLowerCase();
    const pool: DropboxAccountPool =
      selected === "custom_api" ? "custom_api" : "native";
    setCloudService(pool === "custom_api" ? "custom_api" : "dropbox");

    let cloudStatus = "";
    try {
      cloudStatus = (await getCloudConnectionStatus()).trim();
    } catch {
      cloudStatus = "";
    }

    if (pool === "custom_api") {
      setApiStatus(cloudStatus || "Nicht verbunden");
    } else {
      setApiStatus("");
    }

    try {
      const [activeId, rows] = await Promise.all([
        getSetting(dropboxActiveSettingKey(pool), ""),
        listDropboxAccounts(pool),
      ]);
      const row =
        rows.find((r) => r.id === activeId.trim()) ?? rows[0] ?? null;
      setDropboxLabel(row ? dropboxAccountTitle(row) : null);

      if (!row) {
        setDropboxStatus("Nicht verbunden");
        return;
      }

      if (pool === "native" && cloudStatus) {
        // Active cloud is Dropbox — reuse global status (avoids double verify).
        setDropboxStatus(cloudStatus);
        return;
      }

      const status = await verifyDropboxStatus(
        dropboxPoolWhich(pool),
        row.id,
      );
      setDropboxStatus(status.trim() || "Nicht verbunden");
    } catch {
      setDropboxLabel(null);
      setDropboxStatus("Nicht verbunden");
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken, connectionStatus]);

  const apiConnected = isCloudConnected(apiStatus);
  const dropboxConnected = isCloudConnected(dropboxStatus);
  const dropboxName = dropboxLabel ?? "Dropbox";

  return (
    <div className={cn("flex min-w-0 flex-wrap items-center gap-1.5", className)}>
      {cloudService === "custom_api" ? (
        <StatusChip
          Icon={apiConnected ? Server : ServerOff}
          label="Skydive Media"
          title={apiStatus || "Nicht verbunden"}
          tone={toneForStatus(apiStatus)}
        />
      ) : null}
      <StatusChip
        Icon={dropboxConnected ? Cloud : CloudOff}
        label={dropboxName}
        title={`Dropbox: ${dropboxStatus || "Nicht verbunden"}`}
        tone={toneForStatus(dropboxStatus)}
      />
    </div>
  );
}
