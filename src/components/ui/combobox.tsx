import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { ChevronDown } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

export type ComboboxOption = {
  value: string;
  label: string;
  disabled?: boolean;
};

type ComboboxProps = {
  label?: string;
  value: string;
  onChange: (value: string) => void;
  options: readonly ComboboxOption[];
  /** When set, the text field shows/edits this instead of `value` (option values stay separate). */
  inputValue?: string;
  onInputValueChange?: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  id?: string;
  listZIndex?: number;
  hideLabel?: boolean;
  /** Stretch to fill a flex row (e.g. inside SmbUrlField). */
  embedded?: boolean;
  className?: string;
  inputClassName?: string;
  onSelectOption?: (value: string) => void;
  blurOnSelect?: boolean;
  "aria-label"?: string;
};

type ListPos = {
  mode: "fixed" | "absolute";
  top?: number;
  bottom?: number;
  left: number;
  width: number;
  maxHeight: number;
};

const EMPTY_OPTIONS: readonly ComboboxOption[] = [];

/** Editable text input with filtered suggestion list (free text always allowed). */
export function Combobox({
  label,
  value,
  onChange,
  options = EMPTY_OPTIONS,
  inputValue,
  onInputValueChange,
  disabled,
  placeholder,
  id: idProp,
  listZIndex = 80,
  hideLabel = false,
  embedded = false,
  className,
  inputClassName,
  onSelectOption,
  blurOnSelect = false,
  "aria-label": ariaLabel,
}: ComboboxProps) {
  const autoId = useId();
  const id = idProp ?? autoId;
  const listId = `${id}-list`;
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const skipOpenOnFocusRef = useRef(false);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const [filterQuery, setFilterQuery] = useState<string | null>(null);
  const [listPos, setListPos] = useState<ListPos | null>(null);
  const [portalEl, setPortalEl] = useState<HTMLElement | null>(null);

  const entries = useMemo(() => {
    const q = filterQuery === null ? "" : filterQuery.trim().toLowerCase();
    const unique = options.filter((o) => o.value.trim());
    return q
      ? unique.filter(
          (o) =>
            o.label.toLowerCase().includes(q) ||
            o.value.toLowerCase().includes(q),
        )
      : unique;
  }, [options, filterQuery]);

  const selectable = useMemo(
    () => entries.filter((e) => !e.disabled),
    [entries],
  );

  useEffect(() => {
    setHighlight(0);
  }, [entries, filterQuery]);

  useLayoutEffect(() => {
    if (!open || disabled || entries.length === 0) {
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
      const preferredMax = 240;
      const spaceBelow = window.innerHeight - rect.bottom - gap - 8;
      const spaceAbove = rect.top - gap - 8;
      const openUp = spaceBelow < 120 && spaceAbove > spaceBelow;
      const maxHeight = Math.max(
        80,
        Math.min(preferredMax, openUp ? spaceAbove : spaceBelow),
      );

      if (dialog instanceof HTMLElement) {
        const d = dialog.getBoundingClientRect();
        setListPos(
          openUp
            ? {
                mode: "absolute",
                bottom: d.bottom - rect.top + gap,
                left: rect.left - d.left,
                width: rect.width,
                maxHeight,
              }
            : {
                mode: "absolute",
                top: rect.bottom - d.top + gap,
                left: rect.left - d.left,
                width: rect.width,
                maxHeight,
              },
        );
      } else {
        setListPos(
          openUp
            ? {
                mode: "fixed",
                bottom: window.innerHeight - rect.top + gap,
                left: rect.left,
                width: rect.width,
                maxHeight,
              }
            : {
                mode: "fixed",
                top: rect.bottom + gap,
                left: rect.left,
                width: rect.width,
                maxHeight,
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
  }, [open, disabled, entries.length, value]);

  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      const t = e.target as HTMLElement | null;
      if (!t) return;
      if (rootRef.current?.contains(t)) return;
      if (t.closest?.("[data-ams-combobox-list]")) return;
      setOpen(false);
      setFilterQuery(null);
    }
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  function openList() {
    setFilterQuery(null);
    setOpen(true);
  }

  function select(optionValue: string) {
    const hit = options.find((o) => o.value === optionValue);
    if (hit?.disabled) return;
    skipOpenOnFocusRef.current = true;
    onChange(optionValue);
    onSelectOption?.(optionValue);
    setFilterQuery(null);
    setOpen(false);
    if (blurOnSelect) inputRef.current?.blur();
    window.setTimeout(() => {
      skipOpenOnFocusRef.current = false;
    }, 120);
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (disabled) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (!open) openList();
      setHighlight((i) => Math.min(i + 1, Math.max(selectable.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) openList();
      setHighlight((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter" && open && selectable[highlight]) {
      e.preventDefault();
      select(selectable[highlight].value);
    } else if (e.key === "Escape") {
      setOpen(false);
      setFilterQuery(null);
    }
  }

  const listStyle: CSSProperties | undefined = listPos
    ? {
        position: listPos.mode,
        top: listPos.top,
        bottom: listPos.bottom,
        left: listPos.left,
        width: listPos.width,
        maxHeight: listPos.maxHeight,
        zIndex: listZIndex,
        pointerEvents: "auto",
      }
    : undefined;

  const highlightedValue = selectable[highlight]?.value;
  const fieldValue = inputValue ?? value;
  const inputDisplay = filterQuery !== null ? filterQuery : fieldValue;

  const list =
    open && !disabled && entries.length > 0 && listPos && portalEl
      ? createPortal(
          <ul
            id={listId}
            role="listbox"
            data-ams-combobox-list=""
            style={listStyle}
            className="overflow-auto rounded-md border border-border bg-card py-1 shadow-md"
          >
            {entries.map((entry) => {
              const isHighlighted = highlightedValue === entry.value;
              return (
                <li
                  key={entry.value}
                  role="option"
                  aria-selected={isHighlighted}
                  aria-disabled={entry.disabled || undefined}
                >
                  <button
                    type="button"
                    disabled={entry.disabled}
                    className={cn(
                      "flex w-full flex-col px-3 py-1.5 text-left text-sm",
                      entry.disabled
                        ? "cursor-not-allowed text-muted/50"
                        : isHighlighted
                          ? "bg-primary-soft text-foreground"
                          : "text-foreground hover:bg-card-elevated",
                    )}
                    onMouseEnter={() => {
                      if (entry.disabled) return;
                      const idx = selectable.findIndex(
                        (s) => s.value === entry.value,
                      );
                      if (idx >= 0) setHighlight(idx);
                    }}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      if (!entry.disabled) select(entry.value);
                    }}
                  >
                    <span className="font-medium">{entry.label}</span>
                    {entry.label !== entry.value ? (
                      <span className="truncate font-mono text-[10px] text-muted">
                        {entry.value}
                      </span>
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>,
          portalEl,
        )
      : null;

  return (
    <div
      className={cn(
        !hideLabel && label && "space-y-1.5",
        embedded && "min-w-0 flex-1",
        className,
      )}
      ref={rootRef}
    >
      {label ? (
        <Label
          htmlFor={id}
          className={cn("text-xs text-muted", hideLabel && "sr-only")}
        >
          {label}
        </Label>
      ) : null}
      <div
        className={cn("relative min-w-0", embedded ? "w-full flex-1" : "flex-1")}
        ref={triggerRef}
      >
        <Input
          ref={inputRef}
          id={id}
          role="combobox"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-label={ariaLabel}
          autoComplete="off"
          value={inputDisplay}
          disabled={disabled}
          placeholder={placeholder}
          onChange={(e) => {
            setFilterQuery(e.target.value);
            if (onInputValueChange) onInputValueChange(e.target.value);
            else onChange(e.target.value);
            setOpen(true);
          }}
          onFocus={() => {
            if (skipOpenOnFocusRef.current) return;
            openList();
          }}
          onKeyDown={onKeyDown}
          className={cn("w-full pr-9", inputClassName)}
        />
        <button
          type="button"
          tabIndex={-1}
          disabled={disabled}
          aria-label="Vorschläge anzeigen"
          className="absolute top-1/2 right-1 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded text-muted hover:bg-primary-soft hover:text-foreground disabled:pointer-events-none"
          onClick={() => {
            if (open) {
              setOpen(false);
              setFilterQuery(null);
            } else {
              openList();
            }
          }}
        >
          <ChevronDown
            className={cn("h-4 w-4 transition-transform", open && "rotate-180")}
          />
        </button>
      </div>
      {list}
    </div>
  );
}
