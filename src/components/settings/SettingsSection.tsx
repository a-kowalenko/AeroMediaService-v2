import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type Props = {
  title: string;
  description?: string;
  children: ReactNode;
  className?: string;
};

export function SettingsSection({
  title,
  description,
  children,
  className,
}: Props) {
  return (
    <div
      className={cn(
        "space-y-3 rounded-lg border border-border bg-background/60 p-3",
        className,
      )}
    >
      <div>
        <p className="text-xs font-semibold tracking-wide text-muted uppercase">
          {title}
        </p>
        {description ? (
          <p className="mt-1 text-xs leading-relaxed text-muted">{description}</p>
        ) : null}
      </div>
      {children}
    </div>
  );
}
