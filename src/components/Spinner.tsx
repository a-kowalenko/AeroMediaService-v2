import { cn } from "@/lib/utils";

type SpinnerProps = {
  className?: string;
  size?: number;
};

export function Spinner({ className, size = 28 }: SpinnerProps) {
  return (
    <div
      className={cn(
        "inline-block animate-spin rounded-full border-2 border-border border-t-primary",
        className,
      )}
      style={{ width: size, height: size }}
      role="status"
      aria-label="Laden"
    />
  );
}
