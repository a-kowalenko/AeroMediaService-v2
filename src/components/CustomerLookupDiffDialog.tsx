import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ContactFieldKey, IntakeFieldDiff } from "@/lib/customerLookup";
import { mediaFlagsSummary } from "@/lib/customerLookup";
import type { IntakeLookupHit } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type DiffDialogProps = {
  open: boolean;
  diffs: IntakeFieldDiff[];
  resolutions: Partial<Record<ContactFieldKey, "api" | "form">>;
  onResolve: (field: ContactFieldKey, choice: "api" | "form") => void;
  onApplyAllApi: () => void;
  onKeepForm: () => void;
  onConfirm: () => void;
};

export function CustomerLookupDiffDialog({
  open,
  diffs,
  resolutions,
  onResolve,
  onApplyAllApi,
  onKeepForm,
  onConfirm,
}: DiffDialogProps) {
  return (
    <Dialog open={open} onOpenChange={() => {}}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Unterschiede zur Customer-API</DialogTitle>
          <DialogDescription>
            Pro Feld API-Wert übernehmen oder Formular behalten.
          </DialogDescription>
        </DialogHeader>
        <ul className="max-h-[18rem] space-y-2 overflow-y-auto">
          {diffs.map((diff) => {
            const choice = resolutions[diff.field] ?? "form";
            return (
              <li
                key={diff.field}
                className="rounded-lg border border-border bg-card-elevated/50 px-3 py-2.5"
              >
                <p className="mb-1.5 text-xs font-medium text-muted">{diff.label}</p>
                <div className="grid gap-2 sm:grid-cols-2">
                  <button
                    type="button"
                    className={cn(
                      "rounded-md border px-2.5 py-2 text-left text-xs transition-colors",
                      choice === "form"
                        ? "border-primary bg-primary-soft text-foreground"
                        : "border-border text-muted hover:text-foreground",
                    )}
                    onClick={() => onResolve(diff.field, "form")}
                  >
                    <span className="mb-0.5 block text-[10px] uppercase tracking-wide text-muted">
                      Formular
                    </span>
                    {diff.formValue}
                  </button>
                  <button
                    type="button"
                    className={cn(
                      "rounded-md border px-2.5 py-2 text-left text-xs transition-colors",
                      choice === "api"
                        ? "border-primary bg-primary-soft text-foreground"
                        : "border-border text-muted hover:text-foreground",
                    )}
                    onClick={() => onResolve(diff.field, "api")}
                  >
                    <span className="mb-0.5 block text-[10px] uppercase tracking-wide text-muted">
                      API
                    </span>
                    {diff.apiValue}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
        <DialogFooter className="gap-2 sm:justify-between">
          <div className="flex flex-wrap gap-2">
            <Button type="button" variant="secondary" size="sm" onClick={onApplyAllApi}>
              Alle API
            </Button>
            <Button type="button" variant="secondary" size="sm" onClick={onKeepForm}>
              Alle Formular
            </Button>
          </div>
          <Button type="button" onClick={onConfirm}>
            Übernehmen
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

type ChoiceDialogProps = {
  open: boolean;
  handcam: IntakeLookupHit;
  outside: IntakeLookupHit;
  onPick: (hit: IntakeLookupHit) => void;
  onCancel: () => void;
};

export function CustomerLookupChoiceDialog({
  open,
  handcam,
  outside,
  onPick,
  onCancel,
}: ChoiceDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Medientyp wählen</DialogTitle>
          <DialogDescription>
            Für diese IDs gibt es Handcam- und Outside-Buchungen.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          <Button
            type="button"
            variant="secondary"
            className="h-auto flex-col items-start gap-0.5 px-3 py-2.5 text-left"
            onClick={() => onPick(outside)}
          >
            <span className="text-sm font-medium">Outside</span>
            <span className="text-xs text-muted">
              {[outside.vorname, outside.nachname].filter(Boolean).join(" ")}
              {mediaFlagsSummary(outside) ? ` · ${mediaFlagsSummary(outside)}` : ""}
            </span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            className="h-auto flex-col items-start gap-0.5 px-3 py-2.5 text-left"
            onClick={() => onPick(handcam)}
          >
            <span className="text-sm font-medium">Handcam</span>
            <span className="text-xs text-muted">
              {[handcam.vorname, handcam.nachname].filter(Boolean).join(" ")}
              {mediaFlagsSummary(handcam) ? ` · ${mediaFlagsSummary(handcam)}` : ""}
            </span>
          </Button>
        </div>
        <DialogFooter>
          <Button type="button" variant="secondary" onClick={onCancel}>
            Abbrechen
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
