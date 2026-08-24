import toast from "react-hot-toast";
import { AlertTriangle, CheckCircle2, Info, XCircle } from "lucide-react";
import { cn } from "@/lib/utils";

type ToastTone = "success" | "error" | "info" | "warning";

function ToastCard({
  visible,
  tone,
  title,
  message,
  onDismiss,
}: {
  visible: boolean;
  tone: ToastTone;
  title?: string;
  message: string;
  onDismiss: () => void;
}) {
  const Icon =
    tone === "success"
      ? CheckCircle2
      : tone === "error"
        ? XCircle
        : tone === "warning"
          ? AlertTriangle
          : Info;
  return (
    <div
      className={cn(
        "pointer-events-auto flex max-w-sm items-start gap-2.5 rounded-xl border border-border bg-card/95 px-3.5 py-3 shadow-lg backdrop-blur-md transition-all",
        visible ? "opacity-100 translate-y-0" : "opacity-0 translate-y-1",
        tone === "success" && "border-l-4 border-l-success",
        tone === "error" && "border-l-4 border-l-destructive",
        tone === "warning" && "border-l-4 border-l-warning",
        tone === "info" && "border-l-4 border-l-primary",
      )}
      role="status"
    >
      <Icon
        className={cn(
          "mt-0.5 h-4 w-4 shrink-0",
          tone === "success" && "text-success",
          tone === "error" && "text-destructive",
          tone === "warning" && "text-warning",
          tone === "info" && "text-primary",
        )}
        aria-hidden
      />
      <div className="min-w-0 flex-1">
        {title ? (
          <p className="text-sm font-semibold text-foreground">{title}</p>
        ) : null}
        <p className="whitespace-pre-wrap break-words text-sm text-foreground [overflow-wrap:anywhere]">
          {message}
        </p>
      </div>
      <button
        type="button"
        className="shrink-0 text-xs text-muted hover:text-foreground"
        onClick={onDismiss}
      >
        Schließen
      </button>
    </div>
  );
}

export function showAppToast(
  message: string,
  opts?: { title?: string; tone?: ToastTone; durationMs?: number; id?: string },
) {
  const tone = opts?.tone ?? "info";
  const durationMs =
    opts?.durationMs ?? (tone === "error" || tone === "warning" ? 6000 : 4500);
  toast.custom(
    (t) => (
      <ToastCard
        visible={t.visible}
        tone={tone}
        title={opts?.title}
        message={message}
        onDismiss={() => toast.dismiss(t.id)}
      />
    ),
    { duration: durationMs, id: opts?.id },
  );
}
