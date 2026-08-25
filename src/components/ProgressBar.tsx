type ProgressBarProps = {
  percent: number;
  label?: string;
  /** Right-side value under the track, e.g. "32%". */
  detail?: string;
  /** Left-side size under the track, e.g. "1.2 MB / 3.8 MB". */
  sizeDetail?: string;
  /** Compact chip next to the label, e.g. "3/120". */
  badge?: string;
  indeterminate?: boolean;
  /** Thinner bar for secondary parallel-slot rows. */
  size?: "default" | "sm";
};

export function ProgressBar({
  percent,
  label,
  detail,
  sizeDetail,
  badge,
  indeterminate = false,
  size = "default",
}: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(100, percent));
  const gradient =
    "linear-gradient(90deg, var(--ams-progress-from), var(--ams-progress-to))";
  const isSm = size === "sm";
  const trackH = isSm ? "h-1.5" : "h-2";
  const labelClass = isSm
    ? "min-w-0 flex-1 truncate text-[11px] leading-snug text-muted"
    : "min-w-0 flex-1 break-all text-xs font-medium leading-snug text-foreground sm:text-sm";
  const metaClass = isSm
    ? "text-[10px] tabular-nums leading-none text-muted"
    : "text-[11px] tabular-nums leading-none text-muted sm:text-xs";
  const showHeader = Boolean(label || badge);
  const resolvedDetail =
    detail ?? (!indeterminate ? `${clamped.toFixed(0)}%` : undefined);
  const showMeta = Boolean(sizeDetail || resolvedDetail);

  return (
    <div
      className={isSm ? "space-y-1" : "space-y-1.5"}
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : clamped}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={label ?? "Fortschritt"}
    >
      {showHeader ? (
        <div className="flex items-start justify-between gap-2">
          {label ? (
            <p className={labelClass} title={label}>
              {label}
            </p>
          ) : (
            <span />
          )}
          {badge ? (
            <span
              className={
                isSm
                  ? "shrink-0 pt-px text-[10px] tabular-nums text-muted"
                  : "shrink-0 pt-0.5 text-[11px] tabular-nums text-muted sm:text-xs"
              }
            >
              {badge}
            </span>
          ) : null}
        </div>
      ) : null}
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
      {isSm || showMeta ? (
        <div
          className={`flex min-h-[0.875rem] items-baseline justify-between gap-2 ${metaClass}`}
        >
          <span className="min-w-0">{sizeDetail ?? "\u00a0"}</span>
          {resolvedDetail ? (
            <span className="shrink-0 font-medium text-foreground/80">
              {resolvedDetail}
            </span>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
