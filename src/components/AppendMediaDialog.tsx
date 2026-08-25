import { useCallback, useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Check, Film, FolderOpen, ImageIcon, Info, Upload, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { CatStatusChip, type CatStatus } from "@/components/BookingChips";
import {
  cn,
  dropboxPoolActiveSettingKey,
  formatHistoryDropboxAccount,
  historyBookingFlags,
  historyDropboxBinding,
  overlayBookingFlags,
  type HistoryBookingFlags,
} from "@/lib/utils";
import type { AppendCategoryId, AppendFileItem, HistoryEntry } from "@/lib/tauri";
import {
  expandAppendMediaPaths,
  getSetting,
  listDropboxAccounts,
  resolveHistoryBookingFlags,
  type DropboxAccountPool,
} from "@/lib/tauri";

type Props = {
  open: boolean;
  entry: HistoryEntry | null;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (items: AppendFileItem[]) => Promise<void>;
};

type DraftItem = AppendFileItem & { name: string };
type CatGroupId = "handcam" | "outside";

type CatDef = {
  id: AppendCategoryId;
  label: string;
  kindLabel: string;
  group: CatGroupId;
  video: boolean;
  booked: (f: HistoryBookingFlags) => boolean;
  paid: (f: HistoryBookingFlags) => boolean;
};

const CATS: CatDef[] = [
  {
    id: "handcam_foto",
    label: "Handcam Foto",
    kindLabel: "Foto",
    group: "handcam",
    video: false,
    booked: (f) => f.handcam_foto,
    paid: (f) => f.ist_bezahlt_handcam_foto,
  },
  {
    id: "handcam_video",
    label: "Handcam Video",
    kindLabel: "Video",
    group: "handcam",
    video: true,
    booked: (f) => f.handcam_video,
    paid: (f) => f.ist_bezahlt_handcam_video,
  },
  {
    id: "outside_foto",
    label: "Outside Foto",
    kindLabel: "Foto",
    group: "outside",
    video: false,
    booked: (f) => f.outside_foto,
    paid: (f) => f.ist_bezahlt_outside_foto,
  },
  {
    id: "outside_video",
    label: "Outside Video",
    kindLabel: "Video",
    group: "outside",
    video: true,
    booked: (f) => f.outside_video,
    paid: (f) => f.ist_bezahlt_outside_video,
  },
];

const CAT_GROUPS: { id: CatGroupId; label: string }[] = [
  { id: "handcam", label: "Handcam" },
  { id: "outside", label: "Outside" },
];

const VIDEO_EXTS = ["mp4", "mov", "mkv", "avi", "m4v", "webm", "mts", "m2ts"];
const PHOTO_EXTS = ["jpg", "jpeg", "png", "bmp", "tiff", "tif", "webp", "heic", "dng"];

const STATUS_HINT: Record<CatStatus, { text: string; className: string }> = {
  paid: {
    text: "Bezahlt — Dateien werden als Original nachgereicht.",
    className:
      "border-emerald-500/30 bg-emerald-500/10 text-emerald-950 dark:text-emerald-100",
  },
  open: {
    text: "Nicht bezahlt — Originale plus optionale Preview mit Wasserzeichen.",
    className:
      "border-amber-500/30 bg-amber-500/10 text-amber-950 dark:text-amber-100",
  },
  new: {
    text: "Nicht gebucht — Originale plus optionale Preview mit Wasserzeichen.",
    className: "border-border/60 bg-muted/20 text-muted",
  },
};

function basename(path: string): string {
  const n = path.replace(/\\/g, "/").split("/").pop();
  return n || path;
}

function catDef(id: AppendCategoryId): CatDef | undefined {
  return CATS.find((c) => c.id === id);
}

function categoryStatus(f: HistoryBookingFlags, c: CatDef): CatStatus {
  if (!c.booked(f)) return "new";
  if (!c.paid(f)) return "open";
  return "paid";
}

function categoryNotPaid(f: HistoryBookingFlags, id: AppendCategoryId): boolean {
  const c = catDef(id);
  if (!c) return false;
  return !c.booked(f) || !c.paid(f);
}

function itemModeLabel(f: HistoryBookingFlags, item: DraftItem): string {
  const c = catDef(item.category);
  if (!c) return item.preview ? "Original + Preview" : "Original";
  if (c.booked(f) && c.paid(f)) return "Original";
  if (item.preview) return "Original + Preview";
  return "Original";
}

function emptyFlags(): HistoryBookingFlags {
  return {
    handcam_foto: false,
    handcam_video: false,
    outside_foto: false,
    outside_video: false,
    ist_bezahlt_handcam_foto: false,
    ist_bezahlt_handcam_video: false,
    ist_bezahlt_outside_foto: false,
    ist_bezahlt_outside_video: false,
  };
}

export function AppendMediaDialog({
  open,
  entry,
  busy,
  onOpenChange,
  onSubmit,
}: Props) {
  const [category, setCategory] = useState<AppendCategoryId>("handcam_video");
  const [items, setItems] = useState<DraftItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [flags, setFlags] = useState<HistoryBookingFlags>(emptyFlags);
  const [dragOver, setDragOver] = useState(false);
  const [picking, setPicking] = useState(false);
  const [parentAccountBanner, setParentAccountBanner] = useState<string | null>(null);

  const activeCat = useMemo(() => catDef(category), [category]);
  const selectedStatus = activeCat ? categoryStatus(flags, activeCat) : "new";
  const statusHint = STATUS_HINT[selectedStatus];
  const customer =
    `${entry?.first_name ?? ""} ${entry?.last_name ?? ""}`.trim() ||
    entry?.dir_name ||
    "—";

  const groupedItems = useMemo(
    () =>
      CATS.map((c) => ({
        cat: c,
        items: items.filter((i) => i.category === c.id),
      })).filter((g) => g.items.length > 0),
    [items],
  );

  useEffect(() => {
    if (!open || !entry) {
      setParentAccountBanner(null);
      return;
    }
    const binding = historyDropboxBinding(entry);
    if (!binding.amsId) {
      setParentAccountBanner(null);
      return;
    }
    const pool = (binding.pool === "custom_api" ? "custom_api" : "native") as DropboxAccountPool;
    let cancelled = false;
    void (async () => {
      try {
        const activeKey = dropboxPoolActiveSettingKey(pool);
        const [activeId, accounts] = await Promise.all([
          getSetting(activeKey, ""),
          listDropboxAccounts(pool),
        ]);
        if (cancelled) return;
        const active = (activeId ?? "").trim();
        if (!active || active === binding.amsId) {
          setParentAccountBanner(null);
          return;
        }
        const parentRow = accounts.find((a) => a.id === binding.amsId);
        const parentLabel =
          parentRow?.label.trim() ||
          parentRow?.email.trim() ||
          binding.email ||
          binding.amsId;
        const activeRow = accounts.find((a) => a.id === active);
        const activeLabel =
          activeRow?.label.trim() ||
          activeRow?.email.trim() ||
          active;
        setParentAccountBanner(
          `Nachreichen läuft über das Parent-Konto „${parentLabel}“, nicht über das aktive Konto „${activeLabel}“ (${formatHistoryDropboxAccount(entry)}).`,
        );
      } catch {
        if (!cancelled) setParentAccountBanner(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, entry]);

  useEffect(() => {
    if (!open) return;
    const initial = entry ? historyBookingFlags(entry) : emptyFlags();
    const firstBooked =
      CATS.find((c) => c.id === "handcam_video" && c.booked(initial)) ??
      CATS.find((c) => c.booked(initial));
    setFlags(initial);
    setCategory(firstBooked?.id ?? "handcam_video");
    setItems([]);
    setError(null);
    setDragOver(false);

    const id = entry?.id;
    if (!id) return;
    let cancelled = false;
    void resolveHistoryBookingFlags(id, "force")
      .then((resolved) => {
        if (cancelled) return;
        const fromApi = overlayBookingFlags(emptyFlags(), resolved);
        setFlags(fromApi);
        if (!CATS.some((c) => c.booked(initial))) {
          const booked =
            CATS.find((c) => c.id === "handcam_video" && c.booked(fromApi)) ??
            CATS.find((c) => c.booked(fromApi));
          setCategory(booked?.id ?? "handcam_video");
        }
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, entry?.id]);

  const ingestPaths = useCallback(
    async (rawPaths: string[]) => {
      if (busy) return;
      const cat = catDef(category);
      if (!cat || rawPaths.length === 0) return;
      let expanded: string[];
      try {
        expanded = await expandAppendMediaPaths(rawPaths);
      } catch (e) {
        setError(String(e));
        return;
      }
      const previewDefault = categoryNotPaid(flags, category);
      let added = 0;
      let skippedKind = 0;
      setItems((prev) => {
        const known = new Set(prev.map((p) => p.path));
        const batch: DraftItem[] = [];
        skippedKind = 0;
        for (const path of expanded) {
          if (known.has(path)) continue;
          const lower = path.toLowerCase();
          const isVideo = VIDEO_EXTS.some((ext) => lower.endsWith(`.${ext}`));
          const isPhoto = PHOTO_EXTS.some((ext) => lower.endsWith(`.${ext}`));
          if (cat.video && !isVideo) {
            skippedKind += 1;
            continue;
          }
          if (!cat.video && !isPhoto) {
            skippedKind += 1;
            continue;
          }
          known.add(path);
          batch.push({
            path,
            category,
            preview: previewDefault,
            name: basename(path),
          });
        }
        added = batch.length;
        return batch.length === 0 ? prev : [...prev, ...batch];
      });
      if (added === 0) {
        if (expanded.length === 0) {
          setError(
            cat.video
              ? "Keine unterstützten Videos gefunden."
              : "Keine unterstützten Fotos gefunden.",
          );
        } else if (skippedKind > 0) {
          setError(
            cat.video
              ? "Bitte Videos ablegen (aktuelle Kategorie ist Video)."
              : "Bitte Fotos ablegen (aktuelle Kategorie ist Foto).",
          );
        }
        return;
      }
      setError(null);
    },
    [busy, category, flags],
  );

  useEffect(() => {
    if (!open || picking) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          if (busy) {
            setDragOver(false);
            return;
          }
          if (event.payload.type === "enter" || event.payload.type === "over") {
            setDragOver(true);
          } else if (event.payload.type === "leave") {
            setDragOver(false);
          } else if (event.payload.type === "drop") {
            setDragOver(false);
            void ingestPaths(event.payload.paths);
          }
        })
        .then((fn) => {
          if (cancelled) {
            fn();
            return;
          }
          unlisten = fn;
        })
        .catch(() => {
          /* not running inside Tauri webview */
        });
    } catch {
      /* browser preview */
    }
    return () => {
      cancelled = true;
      setDragOver(false);
      unlisten?.();
    };
  }, [open, busy, picking, ingestPaths]);

  async function addFiles() {
    const cat = catDef(category);
    if (!cat) return;
    setPicking(true);
    let selected: string | string[] | null;
    try {
      selected = await openFileDialog({
        title: cat.video ? "Videos wählen" : "Fotos wählen",
        multiple: true,
        filters: [
          {
            name: cat.video ? "Video" : "Fotos",
            extensions: cat.video ? VIDEO_EXTS : PHOTO_EXTS,
          },
        ],
      });
    } catch (e) {
      setError(String(e));
      return;
    } finally {
      setPicking(false);
    }
    const paths = Array.isArray(selected)
      ? selected
      : typeof selected === "string"
        ? [selected]
        : [];
    await ingestPaths(paths);
  }

  async function addFolder() {
    setPicking(true);
    let selected: string | string[] | null;
    try {
      selected = await openFileDialog({
        title: "Ordner wählen",
        directory: true,
        multiple: false,
      });
    } catch (e) {
      setError(String(e));
      return;
    } finally {
      setPicking(false);
    }
    if (typeof selected === "string" && selected) {
      await ingestPaths([selected]);
    }
  }

  function removeItem(path: string) {
    setItems((prev) => prev.filter((i) => i.path !== path));
  }

  function toggleItemPreview(path: string, checked: boolean) {
    setItems((prev) =>
      prev.map((i) => (i.path === path ? { ...i, preview: checked } : i)),
    );
  }

  function setGroupPreview(id: AppendCategoryId, preview: boolean) {
    setItems((prev) =>
      prev.map((i) => (i.category === id ? { ...i, preview } : i)),
    );
  }

  async function submit() {
    if (!entry || items.length === 0 || busy) return;
    const notPaidPhotos = items.filter(
      (i) => categoryNotPaid(flags, i.category) && !catDef(i.category)?.video,
    );
    if (notPaidPhotos.length > 0 && !notPaidPhotos.some((i) => i.preview)) {
      setError(
        "Foto-Produkt ist nicht bezahlt — bitte mindestens ein Foto für das Wasserzeichen auswählen.",
      );
      return;
    }
    const notPaidVideos = items.filter(
      (i) => categoryNotPaid(flags, i.category) && catDef(i.category)?.video,
    );
    if (notPaidVideos.length > 0 && !notPaidVideos.some((i) => i.preview)) {
      setError(
        "Video-Produkt ist nicht bezahlt — bitte mindestens ein Video für die Preview auswählen.",
      );
      return;
    }
    setError(null);
    try {
      await onSubmit(
        items.map(({ path, category, preview }) => ({ path, category, preview })),
      );
    } catch (e) {
      setError(String(e));
    }
  }

  const canSend = !busy && items.length > 0;

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !busy && !picking) onOpenChange(false);
      }}
    >
      <DialogContent
        hideCloseButton
        className="relative z-[55] !flex h-[min(88vh,720px)] w-[min(1100px,96vw)] max-w-none flex-col gap-0 overflow-hidden p-0"
      >
        <header className="grid shrink-0 grid-cols-[minmax(5.5rem,1fr)_auto_minmax(5.5rem,1fr)] items-center border-b border-border/60 px-3 py-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => onOpenChange(false)}
            className="inline-flex items-center justify-self-start rounded-md py-1 pr-2 text-[15px] font-normal text-primary transition hover:brightness-110 disabled:opacity-40"
          >
            Abbrechen
          </button>
          <div className="min-w-0 text-center">
            <DialogTitle className="truncate text-[15px] font-semibold tracking-tight">
              Nachreichen
            </DialogTitle>
            <DialogDescription className="truncate text-[11px] leading-tight">
              {customer}
            </DialogDescription>
          </div>
          <button
            type="button"
            disabled={!canSend}
            onClick={() => void submit()}
            className={cn(
              "justify-self-end rounded-md px-1.5 py-1 text-[15px] font-semibold transition",
              canSend
                ? "text-primary hover:brightness-110"
                : "cursor-not-allowed text-muted/40",
            )}
          >
            {busy ? "Wird nachgereicht…" : "Senden"}
          </button>
        </header>

        {parentAccountBanner ? (
          <div
            className="flex shrink-0 items-start gap-2 border-b border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-950 dark:text-amber-100"
            role="status"
          >
            <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
            <p className="min-w-0 leading-snug">{parentAccountBanner}</p>
          </div>
        ) : null}

        <div className="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[minmax(17.5rem,0.92fr)_minmax(0,1.08fr)]">
          <section className="flex min-h-0 flex-col gap-3 overflow-y-auto border-b border-border/60 p-3 lg:border-b-0 lg:border-r">
            <div>
              <h3 className="px-1 pb-1.5 text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
                Produkt
              </h3>
              <div className="space-y-3" role="radiogroup" aria-label="Produkt für Nachreichen">
                {CAT_GROUPS.map((group) => (
                  <div key={group.id}>
                    <p className="px-1 pb-1 text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
                      {group.label}
                    </p>
                    <div className="overflow-hidden rounded-xl bg-card-elevated ring-1 ring-border/60">
                      {CATS.filter((c) => c.group === group.id).map((c, idx, arr) => {
                        const active = category === c.id;
                        const status = categoryStatus(flags, c);
                        return (
                          <button
                            key={c.id}
                            type="button"
                            role="radio"
                            aria-checked={active}
                            disabled={busy}
                            onClick={() => setCategory(c.id)}
                            className={cn(
                              "flex w-full items-center gap-3 px-3 py-2.5 text-left transition-colors",
                              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                              "disabled:cursor-not-allowed",
                              idx < arr.length - 1 && "border-b border-border/50",
                              active
                                ? "bg-primary-soft"
                                : "hover:bg-black/[0.03] dark:hover:bg-white/[0.04]",
                            )}
                          >
                            <span
                              className={cn(
                                "flex size-8 shrink-0 items-center justify-center rounded-[9px]",
                                active
                                  ? "bg-primary text-primary-foreground"
                                  : "bg-black/8 text-muted dark:bg-white/10",
                              )}
                              aria-hidden
                            >
                              {c.video ? (
                                <Film className="size-4" />
                              ) : (
                                <ImageIcon className="size-4" />
                              )}
                            </span>
                            <span className="min-w-0 flex-1">
                              <span className="block text-[15px] font-medium leading-tight">
                                {c.kindLabel}
                              </span>
                              <span className="mt-1 block">
                                <CatStatusChip status={status} />
                              </span>
                            </span>
                            {active ? (
                              <Check
                                className="size-4 shrink-0 text-primary"
                                strokeWidth={2.5}
                                aria-hidden
                              />
                            ) : null}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                ))}
              </div>
            </div>

            <p
              className={cn(
                "grid grid-cols-[auto_minmax(0,1fr)] items-start gap-2 rounded-xl px-3 py-2 text-xs leading-5 ring-1",
                statusHint.className,
              )}
            >
              {selectedStatus === "paid" ? (
                <Check className="mt-0.5 size-3.5 shrink-0" strokeWidth={2.5} aria-hidden />
              ) : (
                <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden />
              )}
              <span className="leading-5">{statusHint.text}</span>
            </p>

            <div
              className={cn(
                "relative flex min-h-[9.5rem] flex-1 flex-col justify-center overflow-hidden rounded-xl px-3 py-4 text-center transition-[border-color,background-color,box-shadow,transform] duration-200",
                "ring-2 ring-dashed",
                dragOver
                  ? "scale-[1.01] bg-primary-soft ring-primary shadow-[inset_0_0_0_1px] shadow-primary/30"
                  : "bg-card-elevated/70 ring-border hover:ring-primary/40",
                busy && "pointer-events-none opacity-60",
              )}
              role="region"
              aria-label={
                activeCat?.video
                  ? "Videos oder Ordner hierher ziehen"
                  : "Fotos oder Ordner hierher ziehen"
              }
            >
              <div className="relative">
                <div className="mx-auto mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-primary-soft text-primary ring-1 ring-primary/15">
                  {dragOver ? (
                    <Upload className="h-4 w-4 animate-pulse" aria-hidden />
                  ) : activeCat?.video ? (
                    <Film className="h-4 w-4" aria-hidden />
                  ) : (
                    <ImageIcon className="h-4 w-4" aria-hidden />
                  )}
                </div>
                <p className="mb-0.5 text-[13px] font-medium leading-5 text-foreground">
                  {dragOver
                    ? "Loslassen zum Hinzufügen"
                    : activeCat?.video
                      ? "Videos oder Ordner hierher ziehen"
                      : "Fotos oder Ordner hierher ziehen"}
                </p>
                <p className="mb-3 text-[11px] leading-4 text-muted">
                  {activeCat?.video
                    ? "Ordner rekursiv · .mp4, .mov …"
                    : "Ordner rekursiv · .jpg, .png, .webp …"}
                </p>
                <div className="flex flex-wrap items-center justify-center gap-1.5">
                  <Button
                    type="button"
                    size="sm"
                    className="h-7 text-xs"
                    disabled={busy}
                    onClick={() => void addFiles()}
                  >
                    Dateien wählen…
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    className="h-7 text-xs"
                    disabled={busy}
                    onClick={() => void addFolder()}
                  >
                    <FolderOpen className="h-3.5 w-3.5" />
                    Ordner wählen…
                  </Button>
                </div>
              </div>
            </div>

            {error ? <p className="px-1 text-xs text-destructive">{error}</p> : null}
          </section>

          <section className="flex min-h-0 flex-col bg-card-elevated/40">
            <div className="flex shrink-0 items-baseline justify-between gap-2 px-4 py-2.5">
              <h3 className="text-[13px] font-semibold tracking-tight text-foreground">
                Dateien
              </h3>
              <span className="text-[13px] tabular-nums text-muted">
                {items.length === 0
                  ? "Keine"
                  : `${items.length} ${items.length === 1 ? "Datei" : "Dateien"}`}
              </span>
            </div>
            <div className="min-h-0 flex-1 overflow-auto px-3 pb-3">
              {items.length === 0 ? (
                <div className="flex h-full min-h-[12rem] flex-col items-center justify-center px-6 text-center">
                  <div className="mb-3 flex size-12 items-center justify-center rounded-full bg-black/6 text-muted dark:bg-white/8">
                    {activeCat?.video ? (
                      <Film className="size-5" aria-hidden />
                    ) : (
                      <ImageIcon className="size-5" aria-hidden />
                    )}
                  </div>
                  <p className="text-[15px] font-medium text-foreground">Noch keine Dateien</p>
                  <p className="mt-1 max-w-[16rem] text-[13px] leading-5 text-muted">
                    Dateien dem gewählten Produkt zuordnen. Bei offenen oder neuen
                    Optionen pro Datei Preview markieren.
                  </p>
                </div>
              ) : (
                <div className="space-y-3">
                  {groupedItems.map(({ cat, items: groupItems }) => {
                    const showPreview = categoryNotPaid(flags, cat.id);
                    const allPreview =
                      showPreview && groupItems.every((i) => i.preview);
                    return (
                      <div key={cat.id}>
                        <div className="flex items-center justify-between gap-2 px-1 pb-1">
                          <p className="text-[11px] font-semibold tracking-[0.08em] text-muted uppercase">
                            {cat.label}
                            <span className="ml-1.5 tabular-nums font-medium tracking-normal text-muted/80">
                              {groupItems.length}
                            </span>
                          </p>
                          {showPreview ? (
                            <label className="flex cursor-pointer items-center gap-1.5 text-[11px] font-medium text-muted">
                              Preview
                              <Switch
                                checked={allPreview}
                                disabled={busy}
                                onCheckedChange={(v) => setGroupPreview(cat.id, v)}
                              />
                            </label>
                          ) : null}
                        </div>
                        <ul className="overflow-hidden rounded-xl bg-card ring-1 ring-border/60">
                          {groupItems.map((item, idx) => (
                            <li
                              key={item.path}
                              className={cn(
                                "flex items-center gap-3 px-3 py-2",
                                idx < groupItems.length - 1 && "border-b border-border/50",
                              )}
                            >
                              <div className="flex size-11 shrink-0 items-center justify-center rounded-[9px] bg-muted/40 text-muted">
                                {cat.video ? (
                                  <Film className="size-4" />
                                ) : (
                                  <ImageIcon className="size-4" />
                                )}
                              </div>
                              <div className="min-w-0 flex-1">
                                <div className="truncate text-[13px] font-medium" title={item.path}>
                                  {item.name}
                                </div>
                                <div className="truncate text-[11px] text-muted">
                                  {itemModeLabel(flags, item)}
                                </div>
                              </div>
                              {showPreview ? (
                                <Switch
                                  checked={item.preview}
                                  disabled={busy}
                                  aria-label={`Preview für ${item.name}`}
                                  onCheckedChange={(v) =>
                                    toggleItemPreview(item.path, v)
                                  }
                                />
                              ) : null}
                              <button
                                type="button"
                                disabled={busy}
                                className="flex size-7 shrink-0 items-center justify-center rounded-full text-muted transition hover:bg-black/6 hover:text-foreground disabled:opacity-40 dark:hover:bg-white/8"
                                aria-label={`${item.name} entfernen`}
                                onClick={() => removeItem(item.path)}
                              >
                                <X className="size-3.5" strokeWidth={2.25} />
                              </button>
                            </li>
                          ))}
                        </ul>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
