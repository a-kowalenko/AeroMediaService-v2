type ProgressBarProps = {
  percent: number;
  label?: string;
  detail?: string;
  indeterminate?: boolean;
  /** Thinner bar for secondary parallel-slot rows. */
  size?: "default" | "sm";
};

export function ProgressBar({
  percent,
  label,
  detail,
  indeterminate = false,
  size = "default",
}: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(100, percent));
  const gradient =
    "linear-gradient(90deg, var(--ams-progress-from), var(--ams-progress-to))";
  const trackH = size === "sm" ? "h-1.5" : "h-2.5";
  const labelClass =
    size === "sm"
      ? "min-w-0 flex-1 truncate text-xs text-muted"
      : "min-w-0 flex-1 truncate text-sm font-medium text-foreground";
  const detailClass =
    size === "sm"
      ? "shrink-0 text-[10px] tabular-nums text-muted"
      : "shrink-0 text-xs tabular-nums text-muted";

  return (
    <div className="space-y-1.5" role="progressbar" aria-valuenow={indeterminate ? undefined : clamped} aria-valuemin={0} aria-valuemax={100} aria-label={label ?? "Fortschritt"}>
      {(label || detail) && (
        <div className="flex items-baseline justify-between gap-3">
          {label ? (
            <p className={labelClass} title={label}>
              {label}
            </p>
          ) : (
            <span />
          )}
          {detail ? (
            <p className={detailClass}>{detail}</p>
          ) : !indeterminate ? (
            <p className={size === "sm" ? detailClass : "shrink-0 text-sm tabular-nums text-muted"}>
              {clamped.toFixed(0)}%
            </p>
          ) : null}
        </div>
      )}
      <div className={`${trackH} overflow-hidden rounded-full bg-border/60`}>
        {indeterminate ? (
          <div
            className="h-full w-1/3 rounded-full opacity-90"
            style={{
              background: gradient,
              animation: "ams-progress-indeterminate 1.2s ease-in-out infinite",
            }}
          />
        ) : (
          <div
            className="h-full rounded-full transition-[width] duration-500 ease-out"
            style={{ width: `${clamped}%`, background: gradient }}
          />
        )}
      </div>
    </div>
  );
}
