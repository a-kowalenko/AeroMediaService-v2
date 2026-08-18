import { useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import type { AppendCategoryId, AppendFileItem, HistoryEntry } from "@/lib/tauri";

type Props = {
  open: boolean;
  entry: HistoryEntry | null;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (items: AppendFileItem[]) => Promise<void>;
};

type DraftItem = AppendFileItem & { name: string };

type CatDef = {
  id: AppendCategoryId;
  label: string;
  short: string;
  video: boolean;
  booked: (m: Record<string, unknown>) => boolean;
  paid: (m: Record<string, unknown>) => boolean;
};

const CATS: CatDef[] = [
  {
    id: "handcam_video",
    label: "Handcam Video",
    short: "HV",
    video: true,
    booked: (m) => truthy(m.handcam_video),
    paid: (m) => truthy(m.ist_bezahlt_handcam_video),
  },
  {
    id: "handcam_foto",
    label: "Handcam Foto",
    short: "HF",
    video: false,
    booked: (m) => truthy(m.handcam_foto),
    paid: (m) => truthy(m.ist_bezahlt_handcam_foto),
  },
  {
    id: "outside_video",
    label: "Outside Video",
    short: "OV",
    video: true,
    booked: (m) => truthy(m.outside_video),
    paid: (m) => truthy(m.ist_bezahlt_outside_video),
  },
  {
    id: "outside_foto",
    label: "Outside Foto",
    short: "OF",
    video: false,
    booked: (m) => truthy(m.outside_foto),
    paid: (m) => truthy(m.ist_bezahlt_outside_foto),
  },
];

const VIDEO_EXTS = ["mp4", "mov", "mkv", "avi", "m4v", "webm", "mts", "m2ts"];
const PHOTO_EXTS = ["jpg", "jpeg", "png", "bmp", "tiff", "tif", "webp", "heic", "dng"];

function truthy(v: unknown): boolean {
  return v === true || v === 1 || v === "1" || String(v).toLowerCase() === "true";
}

function basename(path: string): string {
  const n = path.replace(/\\/g, "/").split("/").pop();
  return n || path;
}

function extOf(path: string): string {
  const base = basename(path);
  const dot = base.lastIndexOf(".");
  return dot < 0 ? "" : base.slice(dot + 1).toLowerCase();
}

function isVideoPath(path: string): boolean {
  return VIDEO_EXTS.includes(extOf(path));
}

function isPhotoPath(path: string): boolean {
  return PHOTO_EXTS.includes(extOf(path));
}

function catDef(id: AppendCategoryId): CatDef | undefined {
  return CATS.find((c) => c.id === id);
}

function parseMarker(entry: HistoryEntry | null): Record<string, unknown> {
  if (!entry) return {};
  try {
    return JSON.parse(entry.marker_raw || "{}") as Record<string, unknown>;
  } catch {
    const t = (entry.type ?? "").toLowerCase();
    if (t.includes("hand")) {
      return { handcam_video: true, handcam_foto: true };
    }
    if (t.includes("out")) {
      return { outside_video: true, outside_foto: true };
    }
    return {};
  }
}

function categoryUnpaid(marker: Record<string, unknown>, id: AppendCategoryId): boolean {
  const c = catDef(id);
  return Boolean(c?.booked(marker) && !c.paid(marker));
}

function defaultPreviewForCategory(
  marker: Record<string, unknown>,
  id: AppendCategoryId,
): boolean {
  const c = catDef(id);
  if (!c) return false;
  if (!c.booked(marker)) return true;
  return categoryUnpaid(marker, id);
}

function itemModeLabel(marker: Record<string, unknown>, item: DraftItem): string {
  const c = catDef(item.category);
  if (!c) return item.preview ? "Preview" : "Voll";
  if (!c.booked(marker)) return item.preview ? "Preview" : "Voll";
  if (c.paid(marker)) return "Original";
  if (item.preview) return "Original + Preview";
  return "Original";
}

export function AppendMediaDialog({
  open,
  entry,
  busy,
  onOpenChange,
  onSubmit,
}: Props) {
  const [category, setCategory] = useState<AppendCategoryId>("handcam_foto");
  const [unbookedPreviewDefault, setUnbookedPreviewDefault] = useState(true);
  const [items, setItems] = useState<DraftItem[]>([]);
  const [error, setError] = useState<string | null>(null);

  const marker = useMemo(() => parseMarker(entry), [entry]);
  const activeCat = catDef(category);
  const categoryBooked = Boolean(activeCat?.booked(marker));
  const categoryUnpaidActive = categoryUnpaid(marker, category);
  const categoryUnbooked = Boolean(activeCat && !activeCat.booked(marker));

  useEffect(() => {
    if (!open) return;
    const m = parseMarker(entry);
    const first = CATS.find((c) => c.booked(m))?.id ?? "handcam_foto";
    setCategory(first);
    setUnbookedPreviewDefault(!catDef(first)?.booked(m));
    setItems([]);
    setError(null);
  }, [open, entry?.id]);

  async function addFiles() {
    const cat = catDef(category);
    if (!cat) return;
    setError(null);
    let selected: string | string[] | null;
    try {
      selected = await openFileDialog({
        multiple: true,
        title: `${cat.label} wählen`,
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
    }
    if (selected == null) return;
    const paths = Array.isArray(selected)
      ? selected
      : typeof selected === "string"
        ? [selected]
        : [];
    const previewDefault = cat.booked(marker)
      ? defaultPreviewForCategory(marker, category)
      : unbookedPreviewDefault;
    const next: DraftItem[] = [];
    for (const path of paths) {
      if (cat.video && !isVideoPath(path)) continue;
      if (!cat.video && !isPhotoPath(path)) continue;
      if (items.some((i) => i.path === path)) continue;
      next.push({
        path,
        category,
        preview: previewDefault,
        name: basename(path),
      });
    }
    if (next.length === 0) {
      setError("Keine passenden Dateien in der Auswahl (Typ passt nicht zur Option).");
      return;
    }
    setItems((prev) => [...prev, ...next]);
    setError(null);
  }

  function removeItem(path: string) {
    setItems((prev) => prev.filter((i) => i.path !== path));
  }

  function toggleItemPreview(path: string, checked: boolean) {
    setItems((prev) =>
      prev.map((i) => (i.path === path ? { ...i, preview: checked } : i)),
    );
  }

  async function submit() {
    if (!entry || items.length === 0 || busy) return;

    const unbookedFull = items.filter((i) => {
      const cat = catDef(i.category);
      return cat && !cat.booked(marker) && !i.preview;
    });
    if (unbookedFull.length > 0) {
      const ok = window.confirm(
        `${unbookedFull.length} Datei(en) gehen als volles Produkt in den bestehenden Kundenordner, obwohl die Option nicht gebucht war.\n\nFortfahren?`,
      );
      if (!ok) return;
    }

    const unpaidPhotos = items.filter(
      (i) => categoryUnpaid(marker, i.category) && !catDef(i.category)?.video,
    );
    if (unpaidPhotos.length > 0 && !unpaidPhotos.some((i) => i.preview)) {
      setError(
        "Foto-Produkt ist nicht bezahlt — bitte mindestens ein Foto für das Wasserzeichen auswählen.",
      );
      return;
    }

    const unpaidVideos = items.filter(
      (i) => categoryUnpaid(marker, i.category) && catDef(i.category)?.video,
    );
    if (unpaidVideos.length > 0 && !unpaidVideos.some((i) => i.preview)) {
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

  const customer =
    `${entry?.first_name ?? ""} ${entry?.last_name ?? ""}`.trim() ||
    entry?.dir_name ||
    "—";
  const remote = entry?.remote_path.trim() || (entry ? `/${entry.dir_name}` : "");

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !busy) onOpenChange(false);
      }}
    >
      <DialogContent className="z-[55] flex max-h-[min(40rem,calc(100vh-2rem))] max-w-lg flex-col gap-3">
        <DialogHeader>
          <DialogTitle>Dateien nachladen</DialogTitle>
          <DialogDescription>
            Option wählen, dann Dateien aussuchen. Ziel: {remote || "bestehender Cloud-Ordner"}.
            Der Download-Link bleibt; es wird keine Kunden-Nachricht gesendet.
          </DialogDescription>
        </DialogHeader>

        <p className="text-sm text-foreground">{customer}</p>

        <div className="flex flex-wrap gap-1">
          {CATS.map((c) => {
            const isBooked = c.booked(marker);
            const isUnpaid = isBooked && !c.paid(marker);
            const active = category === c.id;
            return (
              <button
                key={c.id}
                type="button"
                disabled={busy}
                onClick={() => setCategory(c.id)}
                className={cn(
                  "inline-flex h-7 items-center rounded border px-2 text-[11px] font-medium",
                  active
                    ? "border-primary/50 bg-primary/15 text-foreground"
                    : "border-border/60 bg-muted/20 text-muted-foreground hover:bg-muted/40",
                )}
                title={
                  !isBooked
                    ? `${c.label} (nicht gebucht)`
                    : isUnpaid
                      ? `${c.label} (nicht bezahlt)`
                      : c.label
                }
              >
                {c.short}
                {!isBooked ? (
                  <span className="ml-1 text-[9px] opacity-70">neu</span>
                ) : isUnpaid ? (
                  <span className="ml-1 text-[9px] opacity-70">offen</span>
                ) : null}
              </button>
            );
          })}
        </div>

        {categoryUnbooked ? (
          <label className="flex items-center justify-between gap-2 rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-sm">
            <span>
              {unbookedPreviewDefault
                ? "Neue Dateien als Preview (Wasserzeichen)"
                : "Neue Dateien als Vollversion"}
              <span className="mt-0.5 block text-[11px] text-muted-foreground">
                Option war nicht gebucht — Preview ist der sichere Default.
              </span>
            </span>
            <Switch
              checked={unbookedPreviewDefault}
              disabled={busy}
              onCheckedChange={setUnbookedPreviewDefault}
            />
          </label>
        ) : categoryUnpaidActive ? (
          <p className="rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs text-muted-foreground">
            Option ist nicht bezahlt: Originale werden hochgeladen. Pro Datei kann
            zusätzlich eine Preview mit Wasserzeichen erzeugt werden — wie beim
            Erstellen und bei ATS-Nachreichung.
          </p>
        ) : categoryBooked ? (
          <p className="rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs text-muted-foreground">
            Gebuchte und bezahlte Option — Dateien werden als Original nachgeladen.
          </p>
        ) : null}

        <div className="flex items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => void addFiles()}
          >
            Dateien wählen…
          </Button>
          <span className="text-xs text-muted-foreground">
            {items.length === 0
              ? `${CATS.find((c) => c.id === category)?.label ?? "Dateien"} auswählen`
              : `${items.length} Datei(en)`}
          </span>
        </div>

        <div className="min-h-0 flex-1 overflow-auto rounded-md border border-border/50">
          {items.length === 0 ? (
            <p className="p-3 text-xs text-muted-foreground">
              Zuerst eine Option wählen (HV / HF / OV / OF), dann die Dateien
              dazu. Bei unbezahlten Optionen pro Datei Preview markieren; Originale
              gehen immer mit hoch.
            </p>
          ) : (
            <ul className="divide-y divide-border/40">
              {items.map((item) => {
                const showPreviewToggle =
                  categoryUnpaid(marker, item.category) ||
                  !catDef(item.category)?.booked(marker);
                const unpaid = categoryUnpaid(marker, item.category);
                return (
                  <li
                    key={item.path}
                    className="flex items-center gap-2 px-2 py-1.5 text-xs"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium" title={item.path}>
                        {item.name}
                      </div>
                      <div className="text-muted-foreground">
                        {catDef(item.category)?.short}
                        {" · "}
                        {itemModeLabel(marker, item)}
                      </div>
                    </div>
                    {showPreviewToggle ? (
                      <label
                        className="flex shrink-0 items-center gap-1.5 text-[10px] text-muted-foreground"
                        title={
                          unpaid
                            ? "Zusätzliche Preview mit Wasserzeichen"
                            : "Als Preview statt Vollversion"
                        }
                      >
                        <Checkbox
                          checked={item.preview}
                          disabled={busy}
                          onCheckedChange={(v) =>
                            toggleItemPreview(item.path, v === true)
                          }
                        />
                        Preview
                      </label>
                    ) : null}
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      className="h-7 px-2"
                      disabled={busy}
                      onClick={() => removeItem(item.path)}
                    >
                      Entf.
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        {error ? <p className="text-xs text-destructive">{error}</p> : null}

        <DialogFooter>
          <Button
            type="button"
            variant="secondary"
            disabled={busy}
            onClick={() => onOpenChange(false)}
          >
            Abbrechen
          </Button>
          <Button
            type="button"
            disabled={busy || items.length === 0}
            onClick={() => void submit()}
          >
            {busy ? "Lade nach…" : "Nachladen"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
