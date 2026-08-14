import { Copy, Minus, Square, X } from "lucide-react";
import { cn } from "@/lib/utils";

type Props = {
  isMaximized: boolean;
  onMinimize: () => void;
  onToggleMaximize: () => void;
  onClose: () => void;
  className?: string;
};

export function TitleBarControls({
  isMaximized,
  onMinimize,
  onToggleMaximize,
  onClose,
  className,
}: Props) {
  return (
    <div
      className={cn("flex h-full shrink-0 items-stretch", className)}
      data-tauri-drag-region="false"
    >
      <button
        type="button"
        aria-label="Minimieren"
        title="Minimieren"
        onClick={onMinimize}
        className="inline-flex w-11 items-center justify-center text-muted transition-colors hover:bg-black/8 hover:text-foreground dark:hover:bg-white/10"
      >
        <Minus className="h-3.5 w-3.5" strokeWidth={2} />
      </button>
      <button
        type="button"
        aria-label={isMaximized ? "Wiederherstellen" : "Maximieren"}
        title={isMaximized ? "Wiederherstellen" : "Maximieren"}
        onClick={onToggleMaximize}
        className="inline-flex w-11 items-center justify-center text-muted transition-colors hover:bg-black/8 hover:text-foreground dark:hover:bg-white/10"
      >
        {isMaximized ? (
          <Copy className="h-3 w-3 -scale-x-100" strokeWidth={2} />
        ) : (
          <Square className="h-3 w-3" strokeWidth={2} />
        )}
      </button>
      <button
        type="button"
        aria-label="Schließen"
        title="Schließen"
        onClick={onClose}
        className="inline-flex w-11 items-center justify-center text-muted transition-colors hover:bg-destructive hover:text-white"
      >
        <X className="h-3.5 w-3.5" strokeWidth={2} />
      </button>
    </div>
  );
}
