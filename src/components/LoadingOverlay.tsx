import { useEffect } from "react";
import { Spinner } from "./Spinner";

type Props = {
  visible: boolean;
  message?: string;
};

/** Port of legacy `utils/loading_overlay.py` — scrim + spinner card. */
export function LoadingOverlay({ visible, message = "" }: Props) {
  useEffect(() => {
    if (!visible) return;
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = prev;
    };
  }, [visible]);

  if (!visible) return null;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/35 backdrop-blur-[1px]"
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <div className="flex min-w-[220px] flex-col items-center gap-3 rounded-lg border border-border bg-card px-8 py-6 shadow-lg">
        <Spinner size={36} />
        {message ? <p className="text-sm text-muted">{message}</p> : null}
      </div>
    </div>
  );
}
