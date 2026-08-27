import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";
import {
  SMB_URL_SCHEME_OPTIONS,
  type SmbUrlScheme,
} from "@/lib/smbUrl";
import { cn } from "@/lib/utils";

type ListPos = {
  mode: "fixed" | "absolute";
  top?: number;
  bottom?: number;
  left: number;
  width: number;
};

type Props = {
  value: SmbUrlScheme;
  onChange: (scheme: SmbUrlScheme) => void;
  labelFor: (scheme: SmbUrlScheme) => string;
  disabled?: boolean;
  "aria-label": string;
  /** Portal list z-index (raise above dialog overlays; default 80). */
  listZIndex?: number;
};

/**
 * Compact protocol prefix picker for SMB/UNC URL fields (ATS parity).
 * Portals into the nearest dialog so the list is not clipped.
 */
export function ServerUrlSchemePicker({
  value,
  onChange,
  labelFor,
  disabled = false,
  "aria-label": ariaLabel,
  listZIndex = 80,
}: Props) {
  const listId = useId();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const [open, setOpen] = useState(false);
  const [listPos, setListPos] = useState<ListPos | null>(null);
  const [portalEl, setPortalEl] = useState<HTMLElement | null>(null);

  useLayoutEffect(() => {
    if (!open || disabled) {
      setListPos(null);
      setPortalEl(null);
      return;
    }

    function updatePos() {
      const el = triggerRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const dialog = el.closest('[role="dialog"]');
      const portal = dialog instanceof HTMLElement ? dialog : document.body;
      setPortalEl(portal);

      const gap = 4;
      const width = Math.max(rect.width, 88);
      const spaceBelow = window.innerHeight - rect.bottom - gap - 8;
      const spaceAbove = rect.top - gap - 8;
      const openUp = spaceBelow < 100 && spaceAbove > spaceBelow;

      if (dialog instanceof HTMLElement) {
        const d = dialog.getBoundingClientRect();
        setListPos(
          openUp
            ? {
                mode: "absolute",
                bottom: d.bottom - rect.top + gap,
                left: rect.left - d.left,
                width,
              }
            : {
                mode: "absolute",
                top: rect.bottom - d.top + gap,
                left: rect.left - d.left,
                width,
              },
        );
      } else {
        setListPos(
          openUp
            ? {
                mode: "fixed",
                bottom: window.innerHeight - rect.top + gap,
                left: rect.left,
                width,
              }
            : {
                mode: "fixed",
                top: rect.bottom + gap,
                left: rect.left,
                width,
              },
        );
      }
    }

    updatePos();
    window.addEventListener("resize", updatePos);
    window.addEventListener("scroll", updatePos, true);
    return () => {
      window.removeEventListener("resize", updatePos);
      window.removeEventListener("scroll", updatePos, true);
    };
  }, [open, disabled]);

  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      const t = e.target as HTMLElement | null;
      if (!t) return;
      if (rootRef.current?.contains(t)) return;
      if (t.closest?.("[data-ams-scheme-list]")) return;
      setOpen(false);
    }
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  const listStyle: CSSProperties | undefined = listPos
    ? {
        position: listPos.mode,
        top: listPos.top,
        bottom: listPos.bottom,
        left: listPos.left,
        width: listPos.width,
        zIndex: listZIndex,
      }
    : undefined;

  return (
    <div ref={rootRef} className="relative shrink-0">
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        onClick={() => {
          if (disabled) return;
          setOpen((v) => !v);
        }}
        className={cn(
          "flex h-9 w-[4.75rem] items-center gap-0.5 rounded-l-md border-0 border-r border-border",
          "bg-card px-1.5 text-xs font-mono text-foreground outline-none",
          "disabled:cursor-not-allowed",
        )}
      >
        <span className="min-w-0 flex-1 truncate text-left">
          {labelFor(value)}
        </span>
        <ChevronDown
          className={cn(
            "h-3 w-3 shrink-0 text-muted transition-transform",
            open && "rotate-180",
          )}
          aria-hidden
        />
      </button>

      {open && !disabled && listPos && portalEl
        ? createPortal(
            <ul
              id={listId}
              role="listbox"
              data-ams-scheme-list=""
              aria-label={ariaLabel}
              style={listStyle}
              className="overflow-hidden rounded-md border border-border bg-card py-1 shadow-md"
            >
              {SMB_URL_SCHEME_OPTIONS.map((opt) => {
                const selected = opt.id === value;
                return (
                  <li key={opt.id} role="presentation">
                    <button
                      type="button"
                      role="option"
                      aria-selected={selected}
                      className={cn(
                        "flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-mono",
                        "hover:bg-primary-soft focus-visible:bg-primary-soft focus-visible:outline-none",
                        selected && "text-primary",
                      )}
                      onClick={() => {
                        onChange(opt.id);
                        setOpen(false);
                      }}
                    >
                      <span className="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                        {selected ? (
                          <Check className="h-3 w-3" strokeWidth={2.5} />
                        ) : null}
                      </span>
                      <span className="min-w-0 truncate">{labelFor(opt.id)}</span>
                    </button>
                  </li>
                );
              })}
            </ul>,
            portalEl,
          )
        : null}
    </div>
  );
}
