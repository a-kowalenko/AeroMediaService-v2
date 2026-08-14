import { cn } from "@/lib/utils";

type StatusLightProps = {
  connected: boolean;
  monitoring: boolean;
  className?: string;
  title?: string;
};

/** Ampel: rot = keine Cloud, gelb = verbunden/Monitor aus, grün = verbunden + Monitoring. */
export function StatusLight({
  connected,
  monitoring,
  className,
  title,
}: StatusLightProps) {
  const color = !connected
    ? "bg-destructive"
    : monitoring
      ? "bg-success"
      : "bg-warning";
  const label = !connected
    ? "Keine Cloud-Verbindung"
    : monitoring
      ? "Verbunden, Monitoring aktiv"
      : "Verbunden, Monitoring inaktiv";
  return (
    <span
      className={cn(
        "relative inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center",
        className,
      )}
      title={title || label}
      role="img"
      aria-label={label}
    >
      {connected && monitoring ? (
        <span
          className="absolute inset-0 animate-ping rounded-full bg-success/40"
          aria-hidden
        />
      ) : null}
      <span
        className={cn(
          "relative inline-block h-3.5 w-3.5 rounded-full ring-2 ring-background",
          color,
        )}
      />
    </span>
  );
}

type StatusDotProps = {
  color: string;
  label?: string;
  className?: string;
};

export function StatusDot({ color, label, className }: StatusDotProps) {
  return (
    <span
      className={cn("inline-block h-2.5 w-2.5 shrink-0 rounded-full", className)}
      style={{ backgroundColor: color }}
      title={label}
      aria-hidden={!label}
    />
  );
}
