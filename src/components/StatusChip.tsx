import { useEffect, useRef, useState, type MouseEvent } from "react";
import {
  AlertCircle,
  Check,
  Clock,
  Loader2,
  MinusCircle,
  Send,
  XCircle,
} from "lucide-react";
import {
  cn,
  overallStatusTone,
  type OverallStatusTone,
  type StatusChannel,
} from "@/lib/utils";

type Props = {
  status: string;
  /** Channel changes meaning of labels like „Gesendet“ (E-Mail OK vs. SMS unterwegs). */
  channel?: StatusChannel;
  /** Show short channel prefix (Upload / E-Mail / SMS / Gesamt). */
  showChannel?: boolean;
  className?: string;
  title?: string;
  compact?: boolean;
  onClick?: (e: MouseEvent<HTMLButtonElement | HTMLSpanElement>) => void;
};

const CHANNEL_LABEL: Record<StatusChannel, string | null> = {
  upload: "Upload",
  email: "E-Mail",
  sms: "SMS",
  overall: "Gesamt",
  generic: null,
};

function StatusIcon({
  tone,
  channel,
  className,
}: {
  tone: OverallStatusTone;
  channel: StatusChannel;
  className?: string;
}) {
  const iconClass = cn("size-3 shrink-0", className);
  switch (tone) {
    case "error":
      return <XCircle className={iconClass} aria-hidden />;
    case "warning":
      return <AlertCircle className={iconClass} aria-hidden />;
    case "active":
      if (channel === "sms") {
        return <Send className={cn(iconClass, "opacity-90")} aria-hidden />;
      }
      return (
        <Loader2
          className={cn(iconClass, "animate-spin [animation-duration:1.4s]")}
          aria-hidden
        />
      );
    case "success":
      return <Check className={iconClass} strokeWidth={2.5} aria-hidden />;
    case "skipped":
      return <MinusCircle className={cn(iconClass, "opacity-80")} aria-hidden />;
    default:
      return <Clock className={cn(iconClass, "opacity-90")} aria-hidden />;
  }
}

function chipClass(tone: OverallStatusTone): string {
  switch (tone) {
    case "error":
      return "border-destructive/40 bg-destructive/10 text-destructive";
    case "warning":
      return "border-warning/45 bg-warning/10 text-warning";
    case "active":
      return "border-primary/40 bg-primary/10 text-primary";
    case "success":
      return "border-success/40 bg-success/10 text-success";
    case "skipped":
      return "border-border/70 bg-muted/30 text-muted";
    default:
      return "border-border/70 bg-muted/40 text-muted";
  }
}

function toneHint(tone: OverallStatusTone, channel: StatusChannel, label: string): string {
  if (tone === "skipped") return `${label} — Kanal nicht genutzt`;
  if (channel === "sms" && tone === "active" && /gesendet/i.test(label)) {
    return `${label} — an Provider übergeben, Zustellung noch offen`;
  }
  if (channel === "email" && tone === "success" && /gesendet/i.test(label)) {
    return `${label} — E-Mail erfolgreich versendet`;
  }
  if (channel === "overall" && /versendet/i.test(label)) {
    return `${label} — Versand erledigt (SMS ggf. ohne Zustellbestätigung)`;
  }
  return label;
}

function describeChip(
  channel: StatusChannel,
  label: string,
  tone: OverallStatusTone,
  title?: string,
): string {
  if (title) return title;
  const hint = toneHint(tone, channel, label);
  const channelName = CHANNEL_LABEL[channel];
  return channelName ? `${channelName}: ${hint}` : hint;
}

/** Compact history / pipeline status chip (ATS handoff-chip style). */
export function StatusChip({
  status,
  channel = "generic",
  showChannel = false,
  className,
  title,
  compact = false,
  onClick,
}: Props) {
  const label = (status || "—").trim() || "—";
  const tone = overallStatusTone(label, channel);
  const channelName = CHANNEL_LABEL[channel];
  const prev = useRef({ label, tone });
  const [successFlash, setSuccessFlash] = useState(false);

  useEffect(() => {
    const prevTone = prev.current.tone;
    if (prev.current.label === label && prevTone === tone) return;
    prev.current = { label, tone };
    if (tone === "success" && prevTone !== "success") {
      setSuccessFlash(true);
      const t = window.setTimeout(() => setSuccessFlash(false), 520);
      return () => window.clearTimeout(t);
    }
    setSuccessFlash(false);
  }, [label, tone]);

  const tip = describeChip(channel, label, tone, title);

  const classes = cn(
    "inline-flex max-w-full items-center gap-1 truncate rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none transition-colors duration-300",
    chipClass(tone),
    tone === "active" && channel !== "sms" && "ams-chip-active",
    successFlash && "ams-chip-success-flash",
    onClick && "cursor-pointer hover:brightness-[1.03]",
    className,
  );

  const body = (
    <>
      <StatusIcon tone={tone} channel={channel} />
      {showChannel && channelName ? (
        <span className="shrink-0 opacity-70">{channelName}</span>
      ) : null}
      <span className={cn("truncate", compact && "max-w-[7rem]")}>{label}</span>
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        className={classes}
        title={tip}
        aria-label={tip}
        onClick={onClick}
      >
        {body}
      </button>
    );
  }

  return (
    <span className={classes} title={tip} aria-label={tip} role="status">
      {body}
    </span>
  );
}

/** Upload / E-Mail / SMS / Gesamt chip row for history detail panes. */
export function HistoryStatusChips({
  entry,
  className,
}: {
  entry: {
    status?: string;
    email_status?: string;
    sms_status?: string;
    overall_status?: string;
  };
  className?: string;
}) {
  return (
    <div className={cn("flex flex-wrap gap-1.5", className)} role="group" aria-label="Status">
      <StatusChip status={entry.status || "—"} channel="upload" showChannel />
      <StatusChip status={entry.email_status || "—"} channel="email" showChannel />
      <StatusChip status={entry.sms_status || "—"} channel="sms" showChannel />
      <StatusChip status={entry.overall_status || "—"} channel="overall" showChannel />
    </div>
  );
}
