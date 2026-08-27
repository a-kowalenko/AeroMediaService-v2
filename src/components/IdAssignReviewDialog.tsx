import {useEffect, useRef, useState} from "react";
import {AlertTriangle, Check} from "lucide-react";
import {Button} from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {Input} from "@/components/ui/input";
import {Label} from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {crewSelectOptions, type CrewMember} from "@/lib/crew";
import {overrideFromPreview} from "@/lib/idAssign";
import {
  previewIdAssign,
  type IdAssignOverride,
  type IdAssignPreview,
} from "@/lib/tauri";

const NONE = "__none__";

type Props = {
  open: boolean;
  /** Initial preview from caller (loaded before opening). */
  initial: IdAssignPreview | null;
  busy?: boolean;
  onCancel: () => void;
  onConfirm: (override: IdAssignOverride) => void;
};

export function IdAssignReviewDialog({
  open,
  initial,
  busy = false,
  onCancel,
  onConfirm,
}: Props) {
  const [preview, setPreview] = useState<IdAssignPreview | null>(null);
  const [tm, setTm] = useState("");
  const [vs, setVs] = useState("");
  const [dropzone, setDropzone] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  /** False until dropdown state is synced from `initial` (avoids empty-override race). */
  const [hydrated, setHydrated] = useState(false);
  const requestRef = useRef(0);

  useEffect(() => {
    if (!open || !initial) {
      setPreview(null);
      setTm("");
      setVs("");
      setDropzone("");
      setHydrated(false);
      setRefreshing(false);
      return;
    }
    setPreview(initial);
    setTm(initial.tandemmaster?.trim() ?? "");
    setVs(initial.videospringer?.trim() ?? "");
    setDropzone(initial.dropzone_suffix?.trim() ?? "");
    setHydrated(true);
  }, [open, initial]);

  useEffect(() => {
    if (!open || !initial || !hydrated) return;

    const override: IdAssignOverride = {
      tandemmaster: tm.trim() || null,
      videospringer: vs.trim() || null,
      dropzone_suffix: dropzone.trim() || null,
    };

    const req = ++requestRef.current;
    setRefreshing(true);
    void previewIdAssign(initial.customer_id, initial.folder_path, override)
      .then((next) => {
        if (req !== requestRef.current) return;
        setPreview(next);
      })
      .catch(() => {
        /* keep last good preview */
      })
      .finally(() => {
        if (req === requestRef.current) setRefreshing(false);
      });

    return () => {
      if (requestRef.current === req) {
        requestRef.current += 1;
      }
    };
  }, [open, initial, hydrated, tm, vs, dropzone]);

  const crew: CrewMember[] = (preview?.crew ?? initial?.crew ?? []).map((m) => ({
    name: m.name,
    tandemmaster: m.tandemmaster,
    videospringer: m.videospringer,
    aliases: m.aliases ?? [],
  }));
  const tmOptions = crewSelectOptions(crew, "tandemmaster", tm);
  const vsOptions = crewSelectOptions(crew, "videospringer", vs);

  function handleConfirm() {
    const base = preview ?? initial;
    if (!base) return;
    onConfirm({
      ...overrideFromPreview({
        ...base,
        tandemmaster: tm.trim() || null,
        videospringer: vs.trim() || null,
        dropzone_suffix: dropzone.trim() || null,
      }),
    });
  }

  const active = preview ?? initial;
  const reasons = active?.review_reasons?.length
    ? active.review_reasons
    : initial?.review_reasons ?? [];

  const detectedGuest = (initial?.customer_label ?? "").trim();
  const detectedTm = (initial?.tandemmaster ?? "").trim();
  const detectedVs = (initial?.videospringer ?? "").trim();
  const sourceName = (initial?.folder_name ?? "").trim();
  const targetName =
    preview?.preview_folder_name || initial?.preview_folder_name || "—";

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !busy) onCancel();
      }}
    >
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Crew &amp; Ordnername prüfen</DialogTitle>
          <DialogDescription>
            {initial
              ? `Zuweisung für ${initial.customer_label} — TM/VS aus dem Ordnernamen sind unsicher oder unvollständig.`
              : "Tandemmaster und Videospringer prüfen (optional)."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {reasons.length > 0 ? (
            <div className="flex gap-2 rounded-md border border-amber-400/40 bg-amber-400/10 px-3 py-2 text-sm text-amber-900 dark:text-amber-100">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <ul className="list-inside list-disc space-y-0.5">
                {reasons.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </div>
          ) : null}

          <div className="rounded-md border border-border bg-card-elevated/40 px-3 py-2.5">
            <p className="text-[10px] font-medium uppercase tracking-wide text-muted">
              Quellordner
            </p>
            <p
              className="mt-1 break-all font-mono text-sm leading-snug text-foreground"
              title={initial?.folder_path || undefined}
            >
              {sourceName || "—"}
            </p>
          </div>

          <div className="rounded-md border border-border bg-card-elevated/40 px-3 py-2.5">
            <p className="text-[10px] font-medium uppercase tracking-wide text-muted">
              Erkannt
            </p>
            <dl className="mt-1.5 space-y-1 text-sm">
              <div className="flex gap-2">
                <dt className="w-28 shrink-0 text-muted">Gast</dt>
                <dd className="min-w-0 font-medium text-foreground">
                  {detectedGuest || "—"}
                </dd>
              </div>
              {detectedTm ? (
                <div className="flex gap-2">
                  <dt className="w-28 shrink-0 text-muted">Tandemmaster</dt>
                  <dd className="min-w-0 text-foreground">{detectedTm}</dd>
                </div>
              ) : null}
              {detectedVs ? (
                <div className="flex gap-2">
                  <dt className="w-28 shrink-0 text-muted">Videospringer</dt>
                  <dd className="min-w-0 text-foreground">{detectedVs}</dd>
                </div>
              ) : null}
            </dl>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="id-assign-tm">Tandemmaster</Label>
            <Select
              value={tm || NONE}
              disabled={busy}
              onValueChange={(v) => setTm(v === NONE ? "" : v)}
            >
              <SelectTrigger id="id-assign-tm">
                <SelectValue placeholder="Optional" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>Kein Tandemmaster</SelectItem>
                {tmOptions.map((name) => (
                  <SelectItem key={name} value={name}>
                    {name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="id-assign-vs">Videospringer</Label>
            <Select
              value={vs || NONE}
              disabled={busy}
              onValueChange={(v) => setVs(v === NONE ? "" : v)}
            >
              <SelectTrigger id="id-assign-vs">
                <SelectValue placeholder="Optional" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>Kein Videospringer</SelectItem>
                {vsOptions.map((name) => (
                  <SelectItem key={name} value={name}>
                    {name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="id-assign-dz">Dropzone (optional)</Label>
            <Input
              id="id-assign-dz"
              value={dropzone}
              disabled={busy}
              maxLength={8}
              placeholder="z. B. G"
              className="font-mono uppercase"
              onChange={(e) =>
                setDropzone(e.target.value.replace(/[^A-Za-z0-9]/g, "").toUpperCase())
              }
            />
          </div>

          <div className="rounded-md border border-border bg-card-elevated/40 px-3 py-2.5">
            <p className="text-[10px] font-medium uppercase tracking-wide text-muted">
              Zielordner {refreshing ? "(aktualisieren…)" : ""}
            </p>
            <p className="mt-1 break-all font-mono text-sm leading-snug text-foreground">
              {targetName}
            </p>
          </div>
        </div>

        <DialogFooter>
          <Button type="button" variant="secondary" disabled={busy} onClick={onCancel}>
            Abbrechen
          </Button>
          <Button type="button" disabled={busy} onClick={handleConfirm}>
            <Check className="h-3.5 w-3.5" />
            {busy ? "Zuweisen…" : "Zuweisen"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
