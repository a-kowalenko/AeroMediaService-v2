import { useCallback, useEffect, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronDown } from "lucide-react";
import { Spinner } from "@/components/Spinner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PasswordInput } from "@/components/ui/password-input";
import { showAppToast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  connectDropbox,
  createDropboxAccount,
  deleteDropboxAccount,
  disconnectDropbox,
  dropboxAccountSecretKeys,
  dropboxPoolWhich,
  finishDropboxOauth,
  getDropboxAccountInfo,
  getSecret,
  getSetting,
  getUploadQueue,
  listDropboxAccounts,
  renameDropboxAccount,
  saveSecret,
  setActiveDropboxAccount,
  verifyDropboxStatus,
  type DropboxAccountInfo,
  type DropboxAccountPool,
  type DropboxAccountRow,
} from "@/lib/tauri";
import { useUiStore } from "@/store/uiStore";

type AccountLive = {
  status: string;
  info: DropboxAccountInfo | null;
  infoError: string | null;
  loading: boolean;
};

type CredEdit = {
  appKey: string;
  appSecret: string;
  loading: boolean;
  dirty: boolean;
};

type Props = {
  open: boolean;
  pool: DropboxAccountPool;
};

type CredDraft = {
  label: string;
  appFolderName: string;
  appKey: string;
  appSecret: string;
};

type EditDraft = {
  label: string;
  appFolderName: string;
};

function displayAppFolderName(
  row: DropboxAccountRow,
  info: DropboxAccountInfo | null | undefined,
): string {
  return row.app_folder_name.trim() || info?.app_name.trim() || "";
}

function formatDropboxBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

function isConnectedStatus(status: string): boolean {
  return status === "Verbunden" || status.startsWith("Verbunden");
}

function accountTitle(row: DropboxAccountRow): string {
  const label = row.label.trim();
  if (label) return label;
  const name = row.display_name.trim();
  if (name) return name;
  const email = row.email.trim();
  if (email) return email;
  return "Dropbox-Konto";
}

function legacySecretKeys(pool: DropboxAccountPool): {
  app_key: string;
  app_secret: string;
} {
  return pool === "native"
    ? { app_key: "db_app_key", app_secret: "db_app_secret" }
    : { app_key: "custom_db_app_key", app_secret: "custom_db_app_secret" };
}

async function persistSecret(key: string, value: string): Promise<void> {
  const trimmed = value.trim();
  if (!trimmed) return;
  await saveSecret(key, trimmed);
}

/** Prefer account keys, then active profile, then legacy pool keys (for connect fallback only). */
async function readAccountCredentials(
  pool: DropboxAccountPool,
  amsId: string,
): Promise<{ appKey: string; appSecret: string }> {
  const keys = dropboxAccountSecretKeys(pool, amsId);
  const [appKey, appSecret] = await Promise.all([
    getSecret(keys.app_key),
    getSecret(keys.app_secret),
  ]);
  return {
    appKey: appKey?.trim() ?? "",
    appSecret: appSecret?.trim() ?? "",
  };
}

async function accountHasAppCredentials(
  pool: DropboxAccountPool,
  amsId: string,
): Promise<boolean> {
  const { appKey, appSecret } = await readAccountCredentials(pool, amsId);
  return Boolean(appKey && appSecret);
}

async function seedAppCredentials(
  pool: DropboxAccountPool,
  amsId: string,
  appKey: string,
  appSecret: string,
): Promise<void> {
  const keys = dropboxAccountSecretKeys(pool, amsId);
  const legacy = legacySecretKeys(pool);
  await persistSecret(keys.app_key, appKey);
  await persistSecret(keys.app_secret, appSecret);
  // Keep legacy mirrors in sync for active Settings/save paths.
  await persistSecret(legacy.app_key, appKey);
  await persistSecret(legacy.app_secret, appSecret);
}

function accountSubtitle(
  row: DropboxAccountRow,
  live: AccountLive | undefined,
  connected: boolean,
): string | null {
  // Live panel already shows name + email when connected.
  if (live?.info) return null;
  const email = row.email.trim();
  if (email) return email;
  if (connected || live?.loading) return null;
  return "Noch nicht verbunden";
}

function DropboxAccountPanel({
  info,
  appFolderName,
  loading,
  error,
  credentials,
}: {
  info: DropboxAccountInfo | null;
  appFolderName: string;
  loading: boolean;
  error: string | null;
  credentials: ReactNode;
}) {
  const allocated = info?.allocated_bytes ?? null;
  const pct =
    info && allocated && allocated > 0
      ? Math.min(100, Math.round((info.used_bytes / allocated) * 1000) / 10)
      : null;
  const storageLabel = info
    ? allocated && allocated > 0
      ? `${formatDropboxBytes(info.used_bytes)} / ${formatDropboxBytes(allocated)}`
      : `${formatDropboxBytes(info.used_bytes)} verwendet`
    : null;
  const photoUrl = info?.profile_photo_url.trim() ?? "";
  const initial =
    (info?.display_name || "?").trim().charAt(0).toUpperCase() || "?";

  return (
    <div className="space-y-2 rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs">
      {loading && !info ? (
        <span className="inline-flex items-center gap-1.5 text-muted">
          <Spinner size={12} className="border-[1.5px]" />
          Kontoinfos werden geladen…
        </span>
      ) : null}
      {error && !info ? (
        <p className="text-warning">Kontoinfos: {error}</p>
      ) : null}
      {info ? (
        <>
          <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
            <span className="text-muted">Konto</span>
            <div className="flex min-w-0 items-center gap-2.5">
              {photoUrl ? (
                <img
                  src={photoUrl}
                  alt=""
                  referrerPolicy="no-referrer"
                  className="h-9 w-9 shrink-0 rounded-full border border-border/60 object-cover bg-muted/40"
                />
              ) : (
                <div
                  className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-border/60 bg-muted/40 text-[11px] font-medium text-muted"
                  aria-hidden
                >
                  {initial}
                </div>
              )}
              <span className="min-w-0 text-right text-foreground">
                {info.display_name || "—"}
                {info.email ? (
                  <span className="block text-muted">{info.email}</span>
                ) : null}
              </span>
            </div>
          </div>
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <span className="text-muted">App-Ordner</span>
            <span className="text-right text-foreground">
              {appFolderName || "—"}
              {info.app_key_hint ? (
                <span className="block font-mono text-muted">{info.app_key_hint}</span>
              ) : null}
            </span>
          </div>
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <span className="text-muted">Token</span>
            <span className={info.token_valid ? "text-success" : "text-warning"}>
              {info.token_valid ? "gültig" : "ungültig"}
            </span>
          </div>
          <div className="space-y-1.5">
            <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
              <span className="text-muted">Speicher</span>
              <span className="text-foreground">
                {storageLabel}
                {pct != null ? ` (${pct} %)` : ""}
              </span>
            </div>
            {pct != null ? (
              <div
                className="h-1.5 overflow-hidden rounded-full bg-muted/50"
                role="progressbar"
                aria-valuenow={pct}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <div
                  className="h-full rounded-full bg-primary/80"
                  style={{ width: `${pct}%` }}
                />
              </div>
            ) : null}
          </div>
        </>
      ) : null}
      {credentials}
    </div>
  );
}

async function promptAuthCode(authorizeUrl: string): Promise<string | null> {
  const ui = useUiStore.getState();
  const proceed = await ui.confirm(
    "Ein Browser-Fenster wird geöffnet, um die App zu autorisieren.\n\n" +
      "Bitte kopieren Sie den angezeigten Code und fügen Sie ihn im nächsten Dialog ein.",
    {
      title: "Dropbox autorisieren",
      primaryLabel: "Browser öffnen",
      secondaryLabel: "Abbrechen",
    },
  );
  if (!proceed) return null;
  try {
    await openUrl(authorizeUrl);
  } catch {
    window.open(authorizeUrl, "_blank", "noopener,noreferrer");
  }
  const code = await ui.prompt("Eingabe-Code von Dropbox:", {
    title: "Autorisierungscode",
    placeholder: "Code einfügen…",
    primaryLabel: "Weiter",
  });
  return code?.trim() || null;
}

function AccountCredentialsDialog({
  open,
  title,
  description,
  draft,
  busy,
  onChange,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  title: string;
  description: string;
  draft: CredDraft;
  busy: boolean;
  onChange: (patch: Partial<CredDraft>) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: () => void;
}) {
  const canSubmit =
    draft.appKey.trim().length > 0 && draft.appSecret.trim().length > 0 && !busy;

  return (
    <Dialog open={open} onOpenChange={(v) => !busy && onOpenChange(v)}>
      <DialogContent className="z-[60] max-w-md" overlayClassName="z-[60]">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-1">
          <div className="space-y-1.5">
            <Label htmlFor="dbx-acc-label">Bezeichnung (optional)</Label>
            <Input
              id="dbx-acc-label"
              value={draft.label}
              disabled={busy}
              placeholder="z. B. Gera"
              onChange={(e) => onChange({ label: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="dbx-acc-app-folder">App-Ordner (optional)</Label>
            <Input
              id="dbx-acc-app-folder"
              value={draft.appFolderName}
              disabled={busy}
              placeholder="z. B. AeroMediaService"
              onChange={(e) => onChange({ appFolderName: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="dbx-acc-key">App Key</Label>
            <PasswordInput
              id="dbx-acc-key"
              autoComplete="off"
              value={draft.appKey}
              disabled={busy}
              onChange={(e) => onChange({ appKey: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="dbx-acc-secret">App Secret</Label>
            <PasswordInput
              id="dbx-acc-secret"
              autoComplete="off"
              value={draft.appSecret}
              disabled={busy}
              onChange={(e) => onChange({ appSecret: e.target.value })}
            />
          </div>
        </div>
        <DialogFooter>
          <Button
            type="button"
            variant="secondary"
            disabled={busy}
            onClick={() => onOpenChange(false)}
          >
            Abbrechen
          </Button>
          <Button type="button" disabled={!canSubmit} onClick={onSubmit}>
            {busy ? "…" : "Anlegen"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EditAccountDialog({
  open,
  draft,
  busy,
  onChange,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  draft: EditDraft;
  busy: boolean;
  onChange: (patch: Partial<EditDraft>) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: () => void;
}) {
  const canSubmit = draft.label.trim().length > 0 && !busy;

  return (
    <Dialog open={open} onOpenChange={(v) => !busy && onOpenChange(v)}>
      <DialogContent className="z-[60] max-w-md" overlayClassName="z-[60]">
        <DialogHeader>
          <DialogTitle>Konto bearbeiten</DialogTitle>
          <DialogDescription>
            Bezeichnung und App-Ordner für dieses Dropbox-Profil.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-1">
          <div className="space-y-1.5">
            <Label htmlFor="dbx-edit-label">Bezeichnung</Label>
            <Input
              id="dbx-edit-label"
              value={draft.label}
              disabled={busy}
              placeholder="z. B. Gera"
              onChange={(e) => onChange({ label: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="dbx-edit-app-folder">App-Ordner</Label>
            <Input
              id="dbx-edit-app-folder"
              value={draft.appFolderName}
              disabled={busy}
              placeholder="z. B. AeroMediaService"
              onChange={(e) => onChange({ appFolderName: e.target.value })}
            />
          </div>
        </div>
        <DialogFooter>
          <Button
            type="button"
            variant="secondary"
            disabled={busy}
            onClick={() => onOpenChange(false)}
          >
            Abbrechen
          </Button>
          <Button type="button" disabled={!canSubmit} onClick={onSubmit}>
            {busy ? "…" : "Speichern"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function DropboxAccountsSection({ open, pool }: Props) {
  const confirm = useUiStore((s) => s.confirm);
  const showError = useUiStore((s) => s.showError);

  const which = dropboxPoolWhich(pool);
  const activeSettingKey =
    pool === "native"
      ? "active_dropbox_account_id"
      : "active_custom_dropbox_account_id";
  const poolLabel = pool === "native" ? "Native" : "Custom-API";

  const [rows, setRows] = useState<DropboxAccountRow[]>([]);
  const [activeId, setActiveId] = useState("");
  const [listLoading, setListLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [liveById, setLiveById] = useState<Record<string, AccountLive>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  const [addOpen, setAddOpen] = useState(false);
  const [addDraft, setAddDraft] = useState<CredDraft>({
    label: "",
    appFolderName: "",
    appKey: "",
    appSecret: "",
  });
  const [editOpen, setEditOpen] = useState(false);
  const [editDraft, setEditDraft] = useState<EditDraft>({
    label: "",
    appFolderName: "",
  });
  const [editRowId, setEditRowId] = useState<string | null>(null);
  const [credsExpanded, setCredsExpanded] = useState<Record<string, boolean>>({});
  const [credEdits, setCredEdits] = useState<Record<string, CredEdit>>({});
  const [credSavingId, setCredSavingId] = useState<string | null>(null);

  const refreshList = useCallback(async () => {
    setListLoading(true);
    setListError(null);
    try {
      const [list, active] = await Promise.all([
        listDropboxAccounts(pool),
        getSetting(activeSettingKey, ""),
      ]);
      setRows(list);
      setActiveId((active ?? "").trim());
      return { list, activeId: (active ?? "").trim() };
    } catch (err) {
      setListError(String(err));
      setRows([]);
      return { list: [] as DropboxAccountRow[], activeId: "" };
    } finally {
      setListLoading(false);
    }
  }, [activeSettingKey, pool]);

  const refreshLive = useCallback(
    async (list: DropboxAccountRow[]) => {
      if (!list.length) {
        setLiveById({});
        return;
      }
      setLiveById((prev) => {
        const next: Record<string, AccountLive> = {};
        for (const row of list) {
          next[row.id] = {
            status: prev[row.id]?.status ?? "…",
            info: prev[row.id]?.info ?? null,
            infoError: null,
            loading: true,
          };
        }
        return next;
      });

      await Promise.all(
        list.map(async (row) => {
          try {
            const status = await verifyDropboxStatus(which, row.id);
            let info: DropboxAccountInfo | null = null;
            let infoError: string | null = null;
            if (isConnectedStatus(status)) {
              try {
                info = await getDropboxAccountInfo(which, row.id);
                if (info?.app_name.trim() && !row.app_folder_name.trim()) {
                  const discovered = info.app_name.trim();
                  setRows((prev) =>
                    prev.map((r) =>
                      r.id === row.id && !r.app_folder_name.trim()
                        ? { ...r, app_folder_name: discovered }
                        : r,
                    ),
                  );
                }
              } catch (e) {
                infoError = String(e);
              }
            }
            setLiveById((prev) => ({
              ...prev,
              [row.id]: { status, info, infoError, loading: false },
            }));
          } catch (e) {
            setLiveById((prev) => ({
              ...prev,
              [row.id]: {
                status: "Verbindungsfehler",
                info: null,
                infoError: String(e),
                loading: false,
              },
            }));
          }
        }),
      );
    },
    [which],
  );

  const reload = useCallback(async () => {
    const { list } = await refreshList();
    await refreshLive(list);
  }, [refreshList, refreshLive]);

  useEffect(() => {
    if (!open) return;
    void reload();
  }, [open, pool, reload]);

  async function openAddDialog() {
    setAddDraft({ label: "", appFolderName: "", appKey: "", appSecret: "" });
    setAddOpen(true);
  }

  function openEditDialog(row: DropboxAccountRow) {
    setEditRowId(row.id);
    setEditDraft({
      label: row.label.trim() || accountTitle(row),
      appFolderName: row.app_folder_name.trim(),
    });
    setEditOpen(true);
  }

  async function loadCredEdits(amsId: string) {
    setCredEdits((prev) => ({
      ...prev,
      [amsId]: {
        appKey: prev[amsId]?.appKey ?? "",
        appSecret: prev[amsId]?.appSecret ?? "",
        loading: true,
        dirty: prev[amsId]?.dirty ?? false,
      },
    }));
    try {
      const { appKey, appSecret } = await readAccountCredentials(pool, amsId);
      setCredEdits((prev) => {
        const current = prev[amsId];
        if (current?.dirty) {
          return {
            ...prev,
            [amsId]: { ...current, loading: false },
          };
        }
        return {
          ...prev,
          [amsId]: { appKey, appSecret, loading: false, dirty: false },
        };
      });
    } catch {
      setCredEdits((prev) => ({
        ...prev,
        [amsId]: {
          appKey: prev[amsId]?.appKey ?? "",
          appSecret: prev[amsId]?.appSecret ?? "",
          loading: false,
          dirty: prev[amsId]?.dirty ?? false,
        },
      }));
    }
  }

  function toggleCreds(amsId: string) {
    setCredsExpanded((prev) => {
      const nextOpen = !prev[amsId];
      if (nextOpen) void loadCredEdits(amsId);
      return { ...prev, [amsId]: nextOpen };
    });
  }

  function patchCredEdit(amsId: string, patch: Partial<Pick<CredEdit, "appKey" | "appSecret">>) {
    setCredEdits((prev) => {
      const cur = prev[amsId] ?? {
        appKey: "",
        appSecret: "",
        loading: false,
        dirty: false,
      };
      return {
        ...prev,
        [amsId]: { ...cur, ...patch, dirty: true, loading: false },
      };
    });
  }

  async function saveCredEdits(amsId: string): Promise<boolean> {
    const edit = credEdits[amsId];
    const appKey = edit?.appKey.trim() ?? "";
    const appSecret = edit?.appSecret.trim() ?? "";
    if (!appKey || !appSecret) {
      showError("App Key und App Secret sind erforderlich.", `Dropbox (${poolLabel})`);
      return false;
    }
    setCredSavingId(amsId);
    try {
      await seedAppCredentials(pool, amsId, appKey, appSecret);
      setCredEdits((prev) => ({
        ...prev,
        [amsId]: {
          appKey,
          appSecret,
          loading: false,
          dirty: false,
        },
      }));
      showAppToast("App Key/Secret für dieses Profil gespeichert.", {
        tone: "success",
        title: `Dropbox (${poolLabel})`,
      });
      return true;
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
      return false;
    } finally {
      setCredSavingId(null);
    }
  }

  async function submitAddAccount() {
    if (!addDraft.appKey.trim() || !addDraft.appSecret.trim()) {
      showError("App Key und App Secret sind erforderlich.", `Dropbox (${poolLabel})`);
      return;
    }
    setAdding(true);
    try {
      const row = await createDropboxAccount(
        pool,
        addDraft.label.trim() || null,
        addDraft.appFolderName.trim() || null,
      );
      await seedAppCredentials(pool, row.id, addDraft.appKey, addDraft.appSecret);
      setAddOpen(false);
      showAppToast(
        `Profil „${accountTitle(row)}“ angelegt. Als Nächstes per OAuth verbinden — gleiche Dropbox-Konto-ID aktualisiert ein bestehendes Profil (kein Duplikat).`,
        { tone: "success", title: `Dropbox (${poolLabel})` },
      );
      await reload();
      setCredsExpanded((prev) => ({ ...prev, [row.id]: true }));
      setCredEdits((prev) => ({
        ...prev,
        [row.id]: {
          appKey: addDraft.appKey.trim(),
          appSecret: addDraft.appSecret.trim(),
          loading: false,
          dirty: false,
        },
      }));
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
    } finally {
      setAdding(false);
    }
  }

  async function runConnect(row: DropboxAccountRow) {
    setBusyId(row.id);
    try {
      const edit = credEdits[row.id];
      if (edit?.dirty && edit.appKey.trim() && edit.appSecret.trim()) {
        await seedAppCredentials(pool, row.id, edit.appKey, edit.appSecret);
        setCredEdits((prev) => ({
          ...prev,
          [row.id]: {
            appKey: edit.appKey.trim(),
            appSecret: edit.appSecret.trim(),
            loading: false,
            dirty: false,
          },
        }));
      } else if (!(await accountHasAppCredentials(pool, row.id))) {
        setCredsExpanded((prev) => ({ ...prev, [row.id]: true }));
        void loadCredEdits(row.id);
        showError(
          "Bitte App Key und App Secret für dieses Konto eintragen und speichern.",
          `Dropbox (${poolLabel})`,
        );
        return;
      }
      let result = await connectDropbox(which, row.id);
      if (result.needs_oauth && result.authorize_url && result.code_verifier) {
        const code = await promptAuthCode(result.authorize_url);
        if (!code) {
          setLiveById((prev) => ({
            ...prev,
            [row.id]: {
              ...(prev[row.id] ?? {
                status: "Nicht verbunden",
                info: null,
                infoError: null,
                loading: false,
              }),
              status: "Nicht verbunden (Abbruch)",
            },
          }));
          return;
        }
        result = await finishDropboxOauth(which, code, result.code_verifier, row.id);
      }
      if (!result.success) {
        showError(result.message || "Dropbox-Verbindung fehlgeschlagen.", `Dropbox (${poolLabel})`);
      } else {
        showAppToast(result.message || "Verbunden.", {
          tone: "success",
          title: `Dropbox (${poolLabel})`,
        });
      }
      await reload();
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
    } finally {
      setBusyId(null);
    }
  }

  async function onDisconnect(row: DropboxAccountRow) {
    setBusyId(row.id);
    try {
      const result = await disconnectDropbox(which, row.id);
      if (!result.success) {
        showError(result.message || "Trennen fehlgeschlagen.", `Dropbox (${poolLabel})`);
      } else {
        showAppToast(result.message || "Getrennt.", {
          tone: "success",
          title: `Dropbox (${poolLabel})`,
        });
      }
      await reload();
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
    } finally {
      setBusyId(null);
    }
  }

  async function onSetActive(row: DropboxAccountRow) {
    if (row.id === activeId) return;
    setBusyId(row.id);
    try {
      const queue = await getUploadQueue();
      if (queue.length > 0) {
        const ok = await confirm(
          `In der Upload-Warteschlange liegen ${queue.length} Job(s).\n\n` +
            "Das aktive Konto gilt nur für neue Jobs. Laufende und wartende Uploads behalten ihr gebundenes Dropbox-Konto.",
          {
            title: "Aktives Konto wechseln",
            primaryLabel: "Trotzdem wechseln",
            secondaryLabel: "Abbrechen",
          },
        );
        if (!ok) return;
      }
      await setActiveDropboxAccount(pool, row.id);
      setActiveId(row.id);
      showAppToast(`„${accountTitle(row)}“ ist jetzt aktiv für neue Jobs.`, {
        tone: "success",
        title: `Dropbox (${poolLabel})`,
      });
      await refreshList();
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
    } finally {
      setBusyId(null);
    }
  }

  async function onRename(row: DropboxAccountRow) {
    openEditDialog(row);
  }

  async function submitEditAccount() {
    if (!editRowId) return;
    const label = editDraft.label.trim();
    if (!label) {
      showError("Bezeichnung darf nicht leer sein.", `Dropbox (${poolLabel})`);
      return;
    }
    setBusyId(editRowId);
    try {
      await renameDropboxAccount(
        editRowId,
        label,
        editDraft.appFolderName.trim(),
      );
      setEditOpen(false);
      setEditRowId(null);
      await refreshList();
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
    } finally {
      setBusyId(null);
    }
  }

  async function onDelete(row: DropboxAccountRow) {
    setBusyId(row.id);
    try {
      const ok = await confirm(
        `Profil „${accountTitle(row)}“ wirklich entfernen?\n\n` +
          "Tokens werden aus dem Keyring gelöscht. Nicht möglich, solange Jobs in der Queue an dieses Konto gebunden sind.",
        {
          title: "Konto entfernen",
          primaryLabel: "Entfernen",
          secondaryLabel: "Abbrechen",
          destructive: true,
        },
      );
      if (!ok) return;
      await deleteDropboxAccount(row.id);
      showAppToast("Profil entfernt.", {
        tone: "success",
        title: `Dropbox (${poolLabel})`,
      });
      await reload();
    } catch (err) {
      showError(String(err), `Dropbox (${poolLabel})`);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs text-muted">
          Mehrere Dropbox-Konten in diesem Pool. Genau eines ist aktiv für{" "}
          <span className="text-foreground">neue</span> Jobs. App Key/Secret gehören
          zum jeweiligen Profil.
        </p>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={adding || busyId !== null}
          onClick={() => void openAddDialog()}
        >
          Konto hinzufügen
        </Button>
      </div>

      {listError ? (
        <div className="rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs text-warning">
          {listError}
        </div>
      ) : null}

      {listLoading && rows.length === 0 ? (
        <div className="rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs text-muted">
          <span className="inline-flex items-center gap-1.5">
            <Spinner size={12} className="border-[1.5px]" />
            Konten werden geladen…
          </span>
        </div>
      ) : null}

      {!listLoading && rows.length === 0 && !listError ? (
        <div className="rounded-md border border-dashed border-border/60 px-3 py-4 text-center text-xs text-muted">
          Noch kein Dropbox-Profil. „Konto hinzufügen“ fragt App Key/Secret ab.
        </div>
      ) : null}

      <div className="space-y-3">
        {rows.map((row) => {
          const live = liveById[row.id];
          const status = live?.status ?? "…";
          const connected = isConnectedStatus(status);
          const isActive = row.id === activeId;
          const busy = busyId === row.id;
          const credsOpen = Boolean(credsExpanded[row.id]);
          const credEdit = credEdits[row.id];
          const subtitle = accountSubtitle(row, live, connected);
          const appFolderName = displayAppFolderName(row, live?.info);

          const credentialsBlock = (
            <div
              className={cn(
                "overflow-hidden rounded-md border border-border/50 bg-background/40",
                live?.info || live?.loading || live?.infoError ? "mt-1" : "",
              )}
            >
              <button
                type="button"
                className="flex w-full items-center justify-between gap-2 px-2.5 py-1.5 text-left text-xs"
                aria-expanded={credsOpen}
                onClick={() => toggleCreds(row.id)}
              >
                <span className="font-medium text-foreground">
                  App Key / App Secret
                  {credEdit?.dirty ? (
                    <span className="ml-1.5 font-normal text-warning">
                      (ungespeichert)
                    </span>
                  ) : null}
                </span>
                <ChevronDown
                  className={cn(
                    "h-4 w-4 shrink-0 text-muted transition-transform",
                    credsOpen ? "" : "-rotate-90",
                  )}
                />
              </button>
              {credsOpen ? (
                <div className="space-y-2 border-t border-border/40 px-2.5 py-2.5">
                  {credEdit?.loading ? (
                    <span className="inline-flex items-center gap-1.5 text-xs text-muted">
                      <Spinner size={12} className="border-[1.5px]" />
                      Credentials werden geladen…
                    </span>
                  ) : (
                    <>
                      <div className="space-y-1.5">
                        <Label htmlFor={`dbx-key-${row.id}`}>App Key</Label>
                        <PasswordInput
                          id={`dbx-key-${row.id}`}
                          autoComplete="off"
                          value={credEdit?.appKey ?? ""}
                          disabled={busy || credSavingId === row.id}
                          onChange={(e) =>
                            patchCredEdit(row.id, { appKey: e.target.value })
                          }
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label htmlFor={`dbx-secret-${row.id}`}>App Secret</Label>
                        <PasswordInput
                          id={`dbx-secret-${row.id}`}
                          autoComplete="off"
                          value={credEdit?.appSecret ?? ""}
                          disabled={busy || credSavingId === row.id}
                          onChange={(e) =>
                            patchCredEdit(row.id, { appSecret: e.target.value })
                          }
                        />
                      </div>
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        disabled={
                          busy ||
                          credSavingId === row.id ||
                          !credEdit?.dirty ||
                          !credEdit.appKey.trim() ||
                          !credEdit.appSecret.trim()
                        }
                        onClick={() => void saveCredEdits(row.id)}
                      >
                        {credSavingId === row.id
                          ? "Speichern…"
                          : "Credentials speichern"}
                      </Button>
                    </>
                  )}
                </div>
              ) : null}
            </div>
          );

          return (
            <div
              key={row.id}
              className="space-y-2 rounded-md border border-border/60 bg-card/40 px-3 py-3"
            >
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div className="min-w-0 space-y-0.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium text-foreground">
                      {accountTitle(row)}
                    </span>
                    {isActive ? (
                      <span className="rounded border border-primary/40 bg-primary/10 px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-primary uppercase">
                        Aktiv
                      </span>
                    ) : null}
                  </div>
                  {subtitle ? (
                    <p className="text-xs text-muted">{subtitle}</p>
                  ) : null}
                  <p className="text-[11px] text-muted">
                    Status:{" "}
                    <span
                      className={
                        connected
                          ? "text-success"
                          : status.includes("fehler") || status.includes("Fehler")
                            ? "text-warning"
                            : "text-foreground"
                      }
                    >
                      {live?.loading ? "prüfen…" : status}
                    </span>
                  </p>
                </div>
              </div>

              <DropboxAccountPanel
                info={live?.info ?? null}
                appFolderName={appFolderName}
                loading={Boolean(live?.loading)}
                error={live?.infoError ?? null}
                credentials={credentialsBlock}
              />

              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  disabled={busy || adding}
                  onClick={() =>
                    void (connected ? onDisconnect(row) : runConnect(row))
                  }
                >
                  {busy ? "…" : connected ? "Trennen" : "Verbinden"}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  disabled={busy || adding || isActive}
                  onClick={() => void onSetActive(row)}
                >
                  Als aktiv
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  disabled={busy || adding}
                  onClick={() => void onRename(row)}
                >
                  Umbenennen
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  disabled={busy || adding}
                  onClick={() => void onDelete(row)}
                >
                  Entfernen
                </Button>
              </div>
            </div>
          );
        })}
      </div>

      <AccountCredentialsDialog
        open={addOpen}
        title="Dropbox-Konto hinzufügen"
        description={`App Key und App Secret gelten nur für dieses ${poolLabel}-Profil.`}
        draft={addDraft}
        busy={adding}
        onChange={(patch) => setAddDraft((d) => ({ ...d, ...patch }))}
        onOpenChange={setAddOpen}
        onSubmit={() => void submitAddAccount()}
      />

      <EditAccountDialog
        open={editOpen}
        draft={editDraft}
        busy={editRowId !== null && busyId === editRowId}
        onChange={(patch) => setEditDraft((d) => ({ ...d, ...patch }))}
        onOpenChange={(open) => {
          if (!open) {
            setEditOpen(false);
            setEditRowId(null);
          }
        }}
        onSubmit={() => void submitEditAccount()}
      />
    </div>
  );
}
