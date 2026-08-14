import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { HistoryEntry } from "@/lib/tauri";
import { lookupShareLink } from "@/lib/tauri";

type Props = {
  entry: HistoryEntry;
  email: string;
  phone: string;
  shareLink: string;
  sandboxWarnings: string[];
  cloudConnected: boolean;
  busy: boolean;
  onClose: () => void;
  onSend: (opts: { sendEmail: boolean; sendSms: boolean; shareLink: string }) => Promise<void>;
};

export function ResendNotificationsDialog({
  entry,
  email,
  phone,
  shareLink,
  sandboxWarnings,
  cloudConnected,
  busy,
  onClose,
  onSend,
}: Props) {
  const [link, setLink] = useState(shareLink.trim());
  const [sendEmail, setSendEmail] = useState(Boolean(email.trim()));
  const [sendSms, setSendSms] = useState(Boolean(phone.trim()));
  const [loadError, setLoadError] = useState("");
  const [loadingLink, setLoadingLink] = useState(false);

  useEffect(() => {
    setLink(shareLink.trim());
    setSendEmail(Boolean(email.trim()));
    setSendSms(Boolean(phone.trim()));
    setLoadError("");
  }, [shareLink, email, phone, entry.id]);

  const customer = `${entry.first_name} ${entry.last_name}`.trim() || "—";

  async function onLoadLink() {
    setLoadError("");
    setLoadingLink(true);
    try {
      const loaded = await lookupShareLink(entry.id);
      setLink(loaded);
    } catch (err) {
      setLoadError(String(err));
    } finally {
      setLoadingLink(false);
    }
  }

  async function onSubmit() {
    await onSend({ sendEmail, sendSms, shareLink: link.trim() });
  }

  return (
    <Dialog
      open
      onOpenChange={(v) => {
        if (!v && !busy) onClose();
      }}
    >
      <DialogContent
        className="z-[55] max-w-md"
        overlayClassName="z-[55]"
        onPointerDownOutside={(e) => {
          if (busy) e.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>Benachrichtigung erneut senden</DialogTitle>
          <DialogDescription>
            Auftrag: {entry.dir_name || "—"} · Kunde: {customer}
          </DialogDescription>
        </DialogHeader>

        {sandboxWarnings.map((warning) => (
          <p key={warning} className="text-sm text-warning">
            ⚠ {warning}
          </p>
        ))}

        <div className="space-y-1.5">
          <Label htmlFor="resend-link">Download-Link</Label>
          <div className="flex gap-2">
            <Input
              id="resend-link"
              type="url"
              placeholder="https://…"
              value={link}
              onChange={(e) => setLink(e.target.value)}
              disabled={busy}
            />
            <Button
              type="button"
              variant="secondary"
              disabled={!cloudConnected || busy || loadingLink}
              title="Link über die verbundene Cloud ermitteln"
              onClick={() => void onLoadLink()}
            >
              {loadingLink ? "Lädt…" : "Aus Cloud laden"}
            </Button>
          </div>
        </div>
        {loadError ? <p className="text-sm text-destructive">{loadError}</p> : null}

        <div className="flex items-center gap-2">
          <Checkbox
            id="resend-email"
            checked={sendEmail}
            disabled={busy}
            onCheckedChange={(v) => setSendEmail(v === true)}
          />
          <Label htmlFor="resend-email" className="font-normal">
            E-Mail senden
          </Label>
        </div>
        <div className="flex items-center gap-2">
          <Checkbox
            id="resend-sms"
            checked={sendSms}
            disabled={busy}
            onCheckedChange={(v) => setSendSms(v === true)}
          />
          <Label htmlFor="resend-sms" className="font-normal">
            SMS senden
          </Label>
        </div>
        <p className="text-xs text-muted">SMS: Jeder Versand verursacht Kosten bei Seven.io.</p>

        <DialogFooter>
          <Button type="button" variant="secondary" disabled={busy} onClick={onClose}>
            Abbrechen
          </Button>
          <Button type="button" disabled={busy} onClick={() => void onSubmit()}>
            {busy ? "Wird gesendet…" : "Jetzt senden"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
