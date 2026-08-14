import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type FocusEvent,
} from "react";
import { Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ThemeToggle } from "@/components/ThemeToggle";
import { LogConsoleToggleButton } from "@/components/LogConsole";
import { useLogStore } from "@/store/logStore";
import { cn } from "@/lib/utils";

const CLOSE_DELAY_MS = 100;

type Props = {
  disabled?: boolean;
  onOpenSettings: () => void;
  className?: string;
};

function prefersCoarsePointer(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia("(hover: none), (pointer: coarse)").matches;
}

export function SettingsCluster({
  disabled,
  onOpenSettings,
  className,
}: Props) {
  const menuId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const closeTimerRef = useRef<number | null>(null);
  const [open, setOpen] = useState(false);
  const unreadErrors = useLogStore((s) => s.unreadErrors);
  const consoleOpen = useLogStore((s) => s.open);

  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current != null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const expand = useCallback(() => {
    clearCloseTimer();
    setOpen(true);
  }, [clearCloseTimer]);

  const collapseSoon = useCallback(() => {
    clearCloseTimer();
    closeTimerRef.current = window.setTimeout(() => {
      setOpen(false);
      closeTimerRef.current = null;
    }, CLOSE_DELAY_MS);
  }, [clearCloseTimer]);

  const collapseNow = useCallback(() => {
    clearCloseTimer();
    setOpen(false);
  }, [clearCloseTimer]);

  useEffect(() => () => clearCloseTimer(), [clearCloseTimer]);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        collapseNow();
        rootRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
      }
    }
    function onPointerDown(e: PointerEvent) {
      const root = rootRef.current;
      if (!root || !(e.target instanceof Node)) return;
      if (!root.contains(e.target)) collapseNow();
    }
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open, collapseNow]);

  function handleSettingsClick() {
    if (disabled) return;
    if (prefersCoarsePointer() && !open) {
      expand();
      return;
    }
    onOpenSettings();
  }

  function handleBlurCapture(e: FocusEvent<HTMLDivElement>) {
    const next = e.relatedTarget;
    if (next instanceof Node && rootRef.current?.contains(next)) return;
    collapseSoon();
  }

  const showClusterBadge = unreadErrors > 0 && !consoleOpen && !open;

  return (
    <div
      ref={rootRef}
      className={cn("relative z-30", className)}
      onMouseEnter={expand}
      onMouseLeave={collapseSoon}
      onFocusCapture={expand}
      onBlurCapture={handleBlurCapture}
    >
      <Button
        type="button"
        variant="secondary"
        size="icon"
        className="relative"
        onClick={handleSettingsClick}
        aria-label="Einstellungen"
        aria-expanded={open}
        aria-controls={menuId}
        title="Einstellungen & mehr"
        disabled={disabled}
      >
        <Settings
          className={cn(
            "h-4 w-4 origin-center transition-transform duration-[320ms] ease-[cubic-bezier(0.2,0.9,0.2,1)] motion-reduce:transition-none",
            open ? "-rotate-[135deg]" : "rotate-0",
          )}
          aria-hidden
        />
        {showClusterBadge ? (
          <span
            className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full bg-destructive ring-2 ring-card"
            aria-hidden
          />
        ) : null}
      </Button>

      <div
        id={menuId}
        role="group"
        aria-label="Schnellaktionen"
        aria-hidden={!open}
        inert={!open ? true : undefined}
        className={cn(
          "ams-settings-cluster-menu absolute right-0 top-full flex w-max flex-col items-end gap-1.5 pt-1.5",
          open ? "pointer-events-auto" : "pointer-events-none",
        )}
        data-open={open ? "true" : "false"}
      >
        <div
          className="ams-settings-cluster-item"
          style={{ ["--ams-cluster-i" as string]: 0 }}
        >
          <ThemeToggle className="shadow-md" />
        </div>
        <div
          className="ams-settings-cluster-item"
          style={{ ["--ams-cluster-i" as string]: 1 }}
        >
          <LogConsoleToggleButton disabled={disabled} className="shadow-md" />
        </div>
      </div>
    </div>
  );
}
