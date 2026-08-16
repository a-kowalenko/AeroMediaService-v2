import { useEffect, useRef, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type Props = {
  open: boolean;
  title?: string;
  message: string;
  autoCloseSecs?: number | null;
  onClose: () => void;
};

export function SuccessDialog({
  open,
  title = "Erfolg",
  message,
  autoCloseSecs = null,
  onClose,
}: Props) {
  const timeoutSecs =
    autoCloseSecs && autoCloseSecs > 0 ? autoCloseSecs : null;
  const [remaining, setRemaining] = useState(timeoutSecs ?? 0);
  const [barActive, setBarActive] = useState(false);
  const closedRef = useRef(false);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open || !timeoutSecs) {
      closedRef.current = false;
      setRemaining(timeoutSecs ?? 0);
      setBarActive(false);
      return;
    }

    closedRef.current = false;
    setRemaining(timeoutSecs);
    setBarActive(false);
    const startRaf = window.requestAnimationFrame(() => setBarActive(true));
    const started = Date.now();
    const id = window.setInterval(() => {
      const left = Math.max(
        0,
        timeoutSecs - Math.floor((Date.now() - started) / 1000),
      );
      setRemaining(left);
      if (left <= 0 && !closedRef.current) {
        closedRef.current = true;
        onCloseRef.current();
      }
    }, 250);
    return () => {
      window.cancelAnimationFrame(startRaf);
      window.clearInterval(id);
    };
  }, [open, timeoutSecs, message, title]);

  function close() {
    if (closedRef.current) return;
    closedRef.current = true;
    onCloseRef.current();
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && close()}>
      <DialogContent
        className="z-[100] max-w-md border-l-4 border-l-success pb-7"
        overlayClassName="z-[100]"
      >
        <DialogHeader>
          <DialogTitle className="text-success">{title}</DialogTitle>
          <DialogDescription className="whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-foreground">
            {message}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button className="shrink-0" onClick={close}>
            OK{timeoutSecs && remaining > 0 ? ` (${remaining}s)` : ""}
          </Button>
        </DialogFooter>
        {timeoutSecs ? (
          <div
            className="pointer-events-none absolute inset-x-0 bottom-0 h-1 overflow-hidden bg-success/15"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={
              barActive
                ? Math.round(((timeoutSecs - remaining) / timeoutSecs) * 100)
                : 0
            }
            aria-label="Automatisches Schließen"
          >
            <div
              className={cn("h-full origin-left bg-success")}
              style={{
                transform: barActive ? "scaleX(1)" : "scaleX(0)",
                transition: barActive
                  ? `transform ${timeoutSecs}s linear`
                  : "none",
              }}
            />
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
