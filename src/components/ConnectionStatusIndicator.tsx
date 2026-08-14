import { Cloud, CloudOff, Radar } from "lucide-react";
import { cn } from "@/lib/utils";
import { isCloudConnected, useAppStore } from "@/store/appStore";

type Props = {
  className?: string;
};

/** Compact header chip — mirrors ATS ServerStatusIndicator for cloud + monitoring. */
export function ConnectionStatusIndicator({ className }: Props) {
  const connectionStatus = useAppStore((s) => s.connectionStatus);
  const monitoring = useAppStore((s) => s.monitoring);
  const uploadJobActive = useAppStore((s) => s.uploadJobActive);
  const connected = isCloudConnected(connectionStatus);

  let label = "Nicht verbunden";
  let tone = "text-muted";
  let Icon = CloudOff;

  if (uploadJobActive) {
    label = "Upload läuft";
    tone = "text-primary";
    Icon = Cloud;
  } else if (connected && monitoring) {
    label = "Monitoring";
    tone = "text-success";
    Icon = Radar;
  } else if (connected) {
    label = "Verbunden";
    tone = "text-warning";
    Icon = Cloud;
  }

  const title = [
    connectionStatus || "Nicht verbunden",
    monitoring ? "Monitoring aktiv" : "Monitoring inaktiv",
    uploadJobActive ? "Upload-Job aktiv" : null,
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-lg border border-border bg-card/80 px-2.5 py-1.5 text-xs shadow-sm",
        tone,
        className,
      )}
      title={title}
    >
      <Icon className="h-3.5 w-3.5" aria-hidden />
      <span>{label}</span>
    </div>
  );
}
