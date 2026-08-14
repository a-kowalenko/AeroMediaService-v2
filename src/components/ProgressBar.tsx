type ProgressBarProps = {
  percent: number;
  label?: string;
  detail?: string;
  indeterminate?: boolean;
};

export function ProgressBar({
  percent,
  label,
  detail,
  indeterminate = false,
}: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(100, percent));
  const gradient =
    "linear-gradient(90deg, var(--ams-progress-from), var(--ams-progress-to))";

  return (
    <div className="space-y-1.5" role="progressbar" aria-valuenow={indeterminate ? undefined : clamped} aria-valuemin={0} aria-valuemax={100} aria-label={label ?? "Fortschritt"}>
      {(label || detail) && (
        <div className="flex items-baseline justify-between gap-3">
          {label ? (
            <p className="min-w-0 flex-1 truncate text-sm font-medium text-foreground" title={label}>
              {label}
            </p>
          ) : (
            <span />
          )}
          {detail ? (
            <p className="shrink-0 text-xs tabular-nums text-muted">{detail}</p>
          ) : !indeterminate ? (
            <p className="shrink-0 text-sm tabular-nums text-muted">{clamped.toFixed(0)}%</p>
          ) : null}
        </div>
      )}
      <div className="h-2.5 overflow-hidden rounded-full bg-border/60">
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
            className="h-full rounded-full transition-[width] duration-300 ease-out"
            style={{ width: `${clamped}%`, background: gradient }}
          />
        )}
      </div>
    </div>
  );
}
