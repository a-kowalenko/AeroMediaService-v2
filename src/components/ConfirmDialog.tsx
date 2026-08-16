import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type ConfirmProps = {
  open: boolean;
  title?: string;
  message: string;
  primaryLabel?: string;
  secondaryLabel?: string;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  open,
  title = "Bestätigen",
  message,
  primaryLabel = "OK",
  secondaryLabel = "Abbrechen",
  destructive = false,
  onConfirm,
  onCancel,
}: ConfirmProps) {
  return (
    <Dialog open={open} onOpenChange={(v) => !v && onCancel()}>
      <DialogContent
        className={
          destructive
            ? "z-[100] max-w-md border-l-4 border-l-destructive"
            : "z-[100] max-w-md border-l-4 border-l-warning"
        }
        overlayClassName="z-[100]"
      >
        <DialogHeader>
          <DialogTitle className={destructive ? "text-destructive" : "text-warning"}>
            {title}
          </DialogTitle>
          <DialogDescription className="whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-foreground">
            {message}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" variant="secondary" onClick={onCancel}>
            {secondaryLabel}
          </Button>
          <Button
            type="button"
            variant={destructive ? "destructive" : "default"}
            onClick={onConfirm}
          >
            {primaryLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

type PromptProps = {
  open: boolean;
  title?: string;
  message: string;
  value: string;
  placeholder?: string;
  hint?: string;
  primaryLabel?: string;
  secondaryLabel?: string;
  onChange: (value: string) => void;
  onConfirm: () => void;
  onCancel: () => void;
};

export function PromptDialog({
  open,
  title = "Eingabe",
  message,
  value,
  placeholder = "",
  hint = "",
  primaryLabel = "OK",
  secondaryLabel = "Abbrechen",
  onChange,
  onConfirm,
  onCancel,
}: PromptProps) {
  return (
    <Dialog open={open} onOpenChange={(v) => !v && onCancel()}>
      <DialogContent
        className="z-[100] max-w-md border-l-4 border-l-primary"
        overlayClassName="z-[100]"
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription className="whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-foreground">
            {message}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-1.5">
          {hint ? (
            <Label htmlFor="ams-prompt-input" className="text-muted">
              {hint}
            </Label>
          ) : null}
          <Input
            id="ams-prompt-input"
            autoFocus
            value={value}
            placeholder={placeholder}
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onConfirm();
              }
            }}
          />
        </div>
        <DialogFooter>
          <Button type="button" variant="secondary" onClick={onCancel}>
            {secondaryLabel}
          </Button>
          <Button type="button" onClick={onConfirm}>
            {primaryLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
