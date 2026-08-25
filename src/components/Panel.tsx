import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type PanelProps = {
  children: ReactNode;
  className?: string;
  title?: string;
  description?: string;
  actions?: ReactNode;
  /** Tighter padding for sidebar cards. */
  compact?: boolean;
};

export function Panel({
  children,
  className,
  title,
  description,
  actions,
  compact = false,
}: PanelProps) {
  return (
    <section
      className={cn(
        "ams-surface rounded-xl shadow-sm backdrop-blur-sm",
        compact ? "p-3.5" : "p-4 sm:p-5",
        className,
      )}
    >
      {(title || actions) && (
        <div
          className={cn(
            "flex flex-wrap items-start justify-between gap-3",
            compact ? "mb-2.5" : "mb-3",
          )}
        >
          <div className="min-w-0">
            {title ? (
              <h2 className="text-sm font-semibold tracking-tight text-foreground sm:text-base">
                {title}
              </h2>
            ) : null}
            {description ? (
              <p
                className="mt-0.5 min-w-0 truncate text-xs leading-relaxed text-muted sm:text-sm"
                title={description}
              >
                {description}
              </p>
            ) : null}
          </div>
          {actions ? (
            <div className="flex shrink-0 flex-wrap items-center gap-2">
              {actions}
            </div>
          ) : null}
        </div>
      )}
      {children}
    </section>
  );
}
