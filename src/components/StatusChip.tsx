import { useEffect, useRef, useState, type MouseEvent } from "react";
import {
  AlertCircle,
  Check,
  Clock,
  Loader2,
  XCircle,
} from "lucide-react";
import { cn, overallStatusTone } from "@/lib/utils";

type Props = {
  status: string;
  className?: string;
  title?: string;
  compact?: boolean;
  onClick?: (e: MouseEvent<HTMLButtonElement | HTMLSpanElement>) => void;
};

function StatusIcon({
  tone,
  className,
}: {
  tone: ReturnType<typeof overallStatusTone>;
  className?: string;
}) {
  const iconClass = cn("size-3 shrink-0", className);
  switch (tone) {
    case "error":
      return <XCircle className={iconClass} aria-hidden />;
    case "warning":
      return <AlertCircle className={iconClass} aria-hidden />;
    case "active":
      return (
        <Loader2
          className={cn(iconClass, "animate-spin [animation-duration:1.4s]")}
          aria-hidden
        />
      );
    case "success":
      return <Check className={iconClass} strokeWidth={2.5} aria-hidden />;
    default:
      return <Clock className={cn(iconClass, "opacity-90")} aria-hidden />;
  }
}

function chipClass(tone: ReturnType<typeof overallStatusTone>): string {
  switch (tone) {
    case "error":
      return "border-destructive/40 bg-destructive/10 text-destructive";
    case "warning":
      return "border-warning/45 bg-warning/10 text-warning";
    case "active":
      return "border-primary/40 bg-primary/10 text-primary";
    case "success":
      return "border-success/40 bg-success/10 text-success";
    default:
      return "border-border/70 bg-muted/40 text-muted";
  }
}

/** Compact history / pipeline status chip (ATS handoff-chip style). */
export function StatusChip({
  status,
  className,
  title,
  compact = false,
  onClick,
}: Props) {
  const label = (status || "—").trim() || "—";
  const tone = overallStatusTone(label);
  const prev = useRef(label);
  const [successFlash, setSuccessFlash] = useState(false);

  useEffect(() => {
    if (prev.current === label) return;
    const was = prev.current;
    prev.current = label;
    if (overallStatusTone(label) === "success" && overallStatusTone(was) !== "success") {
      setSuccessFlash(true);
      const t = window.setTimeout(() => setSuccessFlash(false), 520);
      return () => window.clearTimeout(t);
    }
    setSuccessFlash(false);
  }, [label]);

  const classes = cn(
    "inline-flex max-w-full items-center gap-1 truncate rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none transition-colors duration-300",
    chipClass(tone),
    tone === "active" && "ams-chip-active",
    successFlash && "ams-chip-success-flash",
    onClick && "cursor-pointer hover:brightness-[1.03]",
    className,
  );

  const body = (
    <>
      <StatusIcon tone={tone} />
      <span className={cn("truncate", compact && "max-w-[7rem]")}>{label}</span>
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        className={classes}
        title={title ?? label}
        onClick={onClick}
      >
        {body}
      </button>
    );
  }

  return (
    <span className={classes} title={title ?? label}>
      {body}
    </span>
  );
}
