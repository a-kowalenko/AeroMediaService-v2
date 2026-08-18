import { Check } from "lucide-react";
import { cn, type ProductBadge } from "@/lib/utils";

export type CatStatus = "paid" | "open" | "new";

export function ProductStatusChip({ badge }: { badge: ProductBadge }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-0.5 rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none",
        badge.paid
          ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-900 dark:text-emerald-100"
          : "border-border/60 bg-muted/30 text-muted",
      )}
      title={badge.paid ? `${badge.label} bezahlt` : `${badge.label} nicht bezahlt`}
    >
      {badge.label}
      {badge.paid ? (
        <Check className="size-2.5 shrink-0" strokeWidth={2.5} aria-hidden />
      ) : null}
    </span>
  );
}

export function CatStatusChip({ status }: { status: CatStatus }) {
  if (status === "paid") {
    return (
      <span className="inline-flex items-center gap-0.5 rounded-full border border-emerald-500/40 bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-medium leading-none text-emerald-900 dark:text-emerald-100">
        <Check className="size-2.5 shrink-0" strokeWidth={2.5} aria-hidden />
        Bezahlt
      </span>
    );
  }
  if (status === "open") {
    return (
      <span className="inline-flex items-center rounded-full border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium leading-none text-amber-950 dark:text-amber-100">
        Offen
      </span>
    );
  }
  return (
    <span className="inline-flex items-center rounded-full border border-border/70 bg-muted/30 px-1.5 py-0.5 text-[10px] font-medium leading-none text-muted">
      Nicht gebucht
    </span>
  );
}
