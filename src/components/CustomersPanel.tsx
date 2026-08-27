import { useCallback, useEffect, useRef, useState, type ClipboardEvent, type FormEvent, type ReactNode, type Ref } from "react";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ClipboardPaste,
  FolderInput,
  Hash,
  ListChecks,
  Loader2,
  Pencil,
  RotateCcw,
  Search,
  Trash2,
  UserPlus,
  XCircle,
} from "lucide-react";
import { FolderSelectionModal } from "./FolderSelectionModal";
import { BatchAssignDialog } from "./BatchAssignDialog";
import { IdAssignReviewDialog } from "./IdAssignReviewDialog";
import {
  CustomerLookupChoiceDialog,
  CustomerLookupDiffDialog,
} from "./CustomerLookupDiffDialog";
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
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  applyAllApi,
  applyDiffResolutions,
  contactFieldDiffs,
  customerHasApiIds,
  draftFromForm,
  existingCustomerWarningLabel,
  formToLookupHit,
  isLookupIdPairReady,
  isSameCustomerIdentity,
  LOOKUP_DEBOUNCE_MS,
  mergeLookupIntoForm,
  sanitizeNumericIdInput,
  toDisplayBookingDate,
  toStorageBookingDate,
  type ContactFieldKey,
  type IntakeFieldDiff,
} from "@/lib/customerLookup";
import { cn, formatHistoryDate } from "@/lib/utils";
import {
  listCustomers,
  lookupCustomerIntake,
  previewIdAssign,
  type Customer,
  type IdAssignOverride,
  type IdAssignPreview,
  type IntakeLookupHit,
} from "@/lib/tauri";
import { overrideFromPreview } from "@/lib/idAssign";
import { useCustomerStore, type CustomerFilter } from "@/store/customerStore";
import { useUiStore } from "@/store/uiStore";
import { showAppToast } from "@/lib/toast";

type FormState = {
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
  kunden_id: string;
  booking_id: string;
  booking_date: string;
  typ: string;
  handcam_foto: boolean;
  handcam_video: boolean;
  outside_foto: boolean;
  outside_video: boolean;
  ist_bezahlt_handcam_foto: boolean;
  ist_bezahlt_handcam_video: boolean;
  ist_bezahlt_outside_foto: boolean;
  ist_bezahlt_outside_video: boolean;
  media_option: string;
};

const EMPTY_FORM: FormState = {
  vorname: "",
  nachname: "",
  email: "",
  telefon: "",
  kunden_id: "",
  booking_id: "",
  booking_date: "",
  typ: "",
  handcam_foto: false,
  handcam_video: false,
  outside_foto: false,
  outside_video: false,
  ist_bezahlt_handcam_foto: false,
  ist_bezahlt_handcam_video: false,
  ist_bezahlt_outside_foto: false,
  ist_bezahlt_outside_video: false,
  media_option: "",
};

type ClipboardCustomer = {
  vorname: string;
  nachname: string;
  email: string;
  telefon: string;
  kunden_id: string;
  booking_id: string;
};

/** Stable id for applied/suppressed clipboard payloads (independent of JSON key order). */
function clipboardFingerprint(c: ClipboardCustomer): string {
  return [
    c.vorname,
    c.nachname,
    c.email.toLowerCase(),
    c.telefon,
    c.kunden_id,
    c.booking_id,
  ].join("\0");
}

function sameClipboardCustomer(
  a: ClipboardCustomer | null,
  b: ClipboardCustomer | null,
): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return clipboardFingerprint(a) === clipboardFingerprint(b);
}

function formMatchesClipboard(
  form: Pick<
    FormState,
    "vorname" | "nachname" | "email" | "telefon" | "kunden_id" | "booking_id"
  >,
  c: ClipboardCustomer,
): boolean {
  if (form.vorname.trim() !== c.vorname) return false;
  if (form.nachname.trim() !== c.nachname) return false;
  if (form.email.trim().toLowerCase() !== c.email.toLowerCase()) return false;
  if ((form.telefon.trim() || "") !== (c.telefon || "")) return false;
  if (c.kunden_id && form.kunden_id.trim() !== c.kunden_id) return false;
  if (c.booking_id && form.booking_id.trim() !== c.booking_id) return false;
  return true;
}

async function findExistingCustomer(query: {
  email: string;
  kunden_id?: string;
  booking_id?: string;
}): Promise<Customer | null> {
  const email = query.email.trim();
  const kid = (query.kunden_id ?? "").trim();
  const bid = (query.booking_id ?? "").trim();
  if (!email && !(kid && bid)) return null;
  try {
    const all = await listCustomers("", "all");
    return (
      all.find((c) =>
        isSameCustomerIdentity(
          { email, kunden_id: kid, booking_id: bid },
          c,
        ),
      ) ?? null
    );
  } catch {
    return null;
  }
}

/** Accept string or finite number IDs from clipboard JSON. */
function clipboardIdField(value: unknown): string {
  if (typeof value === "string") return value.trim();
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  return "";
}

function parseCustomerJsonPaste(text: string): ClipboardCustomer | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const data = JSON.parse(trimmed) as Record<string, unknown>;
    if (typeof data !== "object" || data === null || Array.isArray(data)) {
      return null;
    }
    const vorname = data.vorname;
    const nachname = data.name ?? data.nachname;
    const email = data.email;
    const telefon = data.telefon;
    if (
      typeof vorname !== "string" ||
      typeof nachname !== "string" ||
      typeof email !== "string"
    ) {
      return null;
    }
    return {
      vorname: vorname.trim(),
      nachname: nachname.trim(),
      email: email.trim(),
      telefon: typeof telefon === "string" ? telefon.trim() : "",
      kunden_id: clipboardIdField(data.kunden_id),
      booking_id: clipboardIdField(data.booking_id),
    };
  } catch {
    return null;
  }
}

/** `null` = unread (no focus / denied); do not clear UI on that. */
async function readClipboardText(): Promise<string | null> {
  try {
    if (!document.hasFocus()) return null;
    if (navigator.clipboard?.readText) {
      return await navigator.clipboard.readText();
    }
  } catch {
    return null;
  }
  return null;
}

function validateForm(form: FormState): string | null {
  const kid = form.kunden_id.trim();
  const bid = form.booking_id.trim();
  if ((kid && !bid) || (!kid && bid)) {
    return "Kunden-ID und Buchungs-ID müssen beide gesetzt sein oder beide leer bleiben.";
  }
  if (form.vorname.trim().length < 2) return "Vorname ist erforderlich (min. 2 Zeichen).";
  if (form.nachname.trim().length < 2) return "Nachname ist erforderlich (min. 2 Zeichen).";
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(form.email.trim())) {
    return "Bitte eine gültige E-Mail-Adresse eingeben.";
  }
  return null;
}

function hitToForm(hit: IntakeLookupHit, base?: FormState): FormState {
  return {
    ...(base ?? EMPTY_FORM),
    vorname: hit.vorname,
    nachname: hit.nachname,
    email: hit.email,
    telefon: hit.telefon,
    kunden_id: hit.kunden_id,
    booking_id: hit.booking_id,
    booking_date: toDisplayBookingDate(hit.booking_date),
    typ: hit.typ,
    handcam_foto: hit.handcam_foto,
    handcam_video: hit.handcam_video,
    outside_foto: hit.outside_foto,
    outside_video: hit.outside_video,
    // Gebucht = paid; ungebucht = nicht paid (keine separaten Paid-Schalter).
    ist_bezahlt_handcam_foto: hit.handcam_foto,
    ist_bezahlt_handcam_video: hit.handcam_video,
    ist_bezahlt_outside_foto: hit.outside_foto,
    ist_bezahlt_outside_video: hit.outside_video,
    media_option: hit.media_option,
  };
}

function customerToForm(customer: Customer): FormState {
  return {
    vorname: customer.vorname,
    nachname: customer.nachname,
    email: customer.email,
    telefon: customer.telefon,
    kunden_id: customer.kunden_id ?? "",
    booking_id: customer.booking_id ?? "",
    booking_date: toDisplayBookingDate(customer.booking_date ?? ""),
    typ: customer.typ ?? "",
    handcam_foto: Boolean(customer.handcam_foto),
    handcam_video: Boolean(customer.handcam_video),
    outside_foto: Boolean(customer.outside_foto),
    outside_video: Boolean(customer.outside_video),
    ist_bezahlt_handcam_foto: Boolean(customer.handcam_foto),
    ist_bezahlt_handcam_video: Boolean(customer.handcam_video),
    ist_bezahlt_outside_foto: Boolean(customer.outside_foto),
    ist_bezahlt_outside_video: Boolean(customer.outside_video),
    media_option: customer.media_option ?? "",
  };
}

function customerLabel(customer: Pick<Customer, "vorname" | "nachname">): string {
  return `${customer.vorname} ${customer.nachname}`.trim();
}

type LookupUi =
  | { state: "idle" }
  | { state: "searching" }
  | { state: "ok" }
  | { state: "not_found" }
  | { state: "error"; message: string }
  | { state: "choice" }
  | { state: "diffs" }
  | { state: "cancelled" };

/** Folder name from a stored marker path (`…/Job-1/_fertig.txt` → `Job-1`). */
function assignedDirName(assignedPath: string): string {
  const parts = assignedPath
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .filter((part) => part.length > 0);
  if (parts.length === 0) return "";
  const last = parts[parts.length - 1] ?? "";
  if (last === "_fertig.txt" || last === "_in_verarbeitung.txt") {
    return parts[parts.length - 2] ?? last;
  }
  return last;
}

export function CustomersPanel() {
  const items = useCustomerStore((s) => s.items);
  const history = useCustomerStore((s) => s.history);
  const search = useCustomerStore((s) => s.search);
  const filter = useCustomerStore((s) => s.filter);
  const view = useCustomerStore((s) => s.view);
  const openCount = useCustomerStore((s) => s.openCount);
  const highlightId = useCustomerStore((s) => s.highlightId);
  const loading = useCustomerStore((s) => s.loading);
  const setSearch = useCustomerStore((s) => s.setSearch);
  const setFilter = useCustomerStore((s) => s.setFilter);
  const setView = useCustomerStore((s) => s.setView);
  const load = useCustomerStore((s) => s.load);
  const loadHistory = useCustomerStore((s) => s.loadHistory);
  const add = useCustomerStore((s) => s.add);
  const update = useCustomerStore((s) => s.update);
  const remove = useCustomerStore((s) => s.remove);
  const setProcessed = useCustomerStore((s) => s.setProcessed);
  const assign = useCustomerStore((s) => s.assign);
  const confirm = useUiStore((s) => s.confirm);

  const [intakeOpen, setIntakeOpen] = useState(false);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [formError, setFormError] = useState("");
  const [formBusy, setFormBusy] = useState(false);
  const [assignAfterSave, setAssignAfterSave] = useState(false);
  const [intakeSavedCount, setIntakeSavedCount] = useState(0);
  const [intakeFocusKey, setIntakeFocusKey] = useState(0);
  const [clipboardCustomer, setClipboardCustomer] = useState<ClipboardCustomer | null>(
    null,
  );
  const [clipboardExisting, setClipboardExisting] = useState<Customer | null>(null);
  const [intakeExisting, setIntakeExisting] = useState<Customer | null>(null);
  const lastAppliedClipboardRef = useRef("");
  const kundenIdRef = useRef<HTMLInputElement>(null);
  const formRef = useRef(form);
  formRef.current = form;

  const [exportingId, setExportingId] = useState<string | null>(null);
  const [exportingLabel, setExportingLabel] = useState("");
  const [exportingVorname, setExportingVorname] = useState("");
  const [exportingNachname, setExportingNachname] = useState("");
  const [exportingEmail, setExportingEmail] = useState("");
  const [exportingCustomer, setExportingCustomer] = useState<Customer | null>(null);
  const [assignBusy, setAssignBusy] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [reviewPreview, setReviewPreview] = useState<IdAssignPreview | null>(null);
  const [reviewFolderPath, setReviewFolderPath] = useState("");
  const [editing, setEditing] = useState<Customer | null>(null);
  const [editForm, setEditForm] = useState<FormState>(EMPTY_FORM);
  const [editError, setEditError] = useState("");
  const [editBusy, setEditBusy] = useState(false);
  const [batchOpen, setBatchOpen] = useState(false);
  const [lookupBusy, setLookupBusy] = useState(false);
  const [lookupUi, setLookupUi] = useState<LookupUi>({ state: "idle" });
  const [lookupAppliedKey, setLookupAppliedKey] = useState("");
  const lookupRequestRef = useRef(0);
  const lookupSinkRef = useRef<"intake" | "edit">("intake");

  const [editLookupBusy, setEditLookupBusy] = useState(false);
  const [editLookupUi, setEditLookupUi] = useState<LookupUi>({ state: "idle" });
  const [editLookupAppliedKey, setEditLookupAppliedKey] = useState("");
  const editFormRef = useRef(editForm);
  editFormRef.current = editForm;

  const [diffOpen, setDiffOpen] = useState(false);
  const [diffApi, setDiffApi] = useState<IntakeLookupHit | null>(null);
  const [diffList, setDiffList] = useState<IntakeFieldDiff[]>([]);
  const [diffResolutions, setDiffResolutions] = useState<
    Partial<Record<ContactFieldKey, "api" | "form">>
  >({});

  const [choiceOpen, setChoiceOpen] = useState(false);
  const [choiceHandcam, setChoiceHandcam] = useState<IntakeLookupHit | null>(null);
  const [choiceOutside, setChoiceOutside] = useState<IntakeLookupHit | null>(null);

  function clearExporting() {
    setExportingId(null);
    setExportingLabel("");
    setExportingVorname("");
    setExportingNachname("");
    setExportingEmail("");
    setExportingCustomer(null);
  }

  function startAssign(customer: Customer) {
    setExportingId(customer.id);
    setExportingLabel(customerLabel(customer));
    setExportingVorname(customer.vorname);
    setExportingNachname(customer.nachname);
    setExportingEmail(customer.email);
    setExportingCustomer(customer);
  }

  async function runAssign(
    customerId: string,
    folderPath: string,
    idOverride?: IdAssignOverride | null,
  ) {
    setAssignBusy(true);
    try {
      await assign(customerId, folderPath, idOverride);
      clearExporting();
      setReviewOpen(false);
      setReviewPreview(null);
      setReviewFolderPath("");
    } catch {
      /* store toasts */
    } finally {
      setAssignBusy(false);
    }
  }

  async function onAssign(folderPath: string) {
    if (!exportingId) return;
    const customer =
      exportingCustomer ??
      items.find((c) => c.id === exportingId) ??
      null;

    if (customer && customerHasApiIds(customer)) {
      setAssignBusy(true);
      try {
        const preview = await previewIdAssign(exportingId, folderPath);
        if (preview.needs_review || (preview.vs_required && !preview.videospringer)) {
          setReviewPreview(preview);
          setReviewFolderPath(folderPath);
          setExportingId(null);
          setReviewOpen(true);
          return;
        }
        await assign(exportingId, folderPath, overrideFromPreview(preview));
        clearExporting();
      } catch (err) {
        showAppToast(String(err), { tone: "error", title: "Zuweisung" });
      } finally {
        setAssignBusy(false);
      }
      return;
    }

    await runAssign(exportingId, folderPath);
  }

  const checkClipboard = useCallback(async () => {
    try {
      const text = await readClipboardText();
      // Unfocused / denied reads: keep previous banner state (avoids layout jump).
      if (text === null) return;

      const trimmed = text.trim();
      const parsed = parseCustomerJsonPaste(trimmed);
      if (parsed) {
        const fp = clipboardFingerprint(parsed);
        if (fp === lastAppliedClipboardRef.current) {
          setClipboardCustomer((prev) => (prev === null ? prev : null));
          return;
        }
        setClipboardCustomer((prev) =>
          sameClipboardCustomer(prev, parsed) ? prev : parsed,
        );
        return;
      }
      setClipboardCustomer((prev) => (prev === null ? prev : null));
    } catch {
      /* keep previous */
    }
  }, []);

  useEffect(() => {
    void load();
    void loadHistory();
  }, [load, loadHistory]);

  useEffect(() => {
    void checkClipboard();
    const onFocus = () => void checkClipboard();
    const onVisibility = () => {
      if (document.visibilityState === "visible") void checkClipboard();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    // Slower poll + skip when unfocused (handled in read) reduces banner flicker.
    const interval = window.setInterval(() => void checkClipboard(), 2500);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
      window.clearInterval(interval);
    };
  }, [checkClipboard]);

  useEffect(() => {
    if (!intakeOpen) return;
    const id = window.setTimeout(() => kundenIdRef.current?.focus(), 40);
    return () => window.clearTimeout(id);
  }, [intakeOpen, intakeFocusKey]);

  function setLookupUiForSink(sink: "intake" | "edit", ui: LookupUi) {
    if (sink === "edit") setEditLookupUi(ui);
    else setLookupUi(ui);
  }

  function setLookupAppliedKeyForSink(sink: "intake" | "edit", key: string) {
    if (sink === "edit") setEditLookupAppliedKey(key);
    else setLookupAppliedKey(key);
  }

  function setLookupBusyForSink(sink: "intake" | "edit", busy: boolean) {
    if (sink === "edit") setEditLookupBusy(busy);
    else setLookupBusy(busy);
  }

  function applyHitToForm(hit: IntakeLookupHit, sink: "intake" | "edit" = lookupSinkRef.current) {
    lookupSinkRef.current = sink;
    const current = sink === "edit" ? editFormRef.current : formRef.current;
    const formHit = formToLookupHit(current);
    const diffs = contactFieldDiffs(formHit, hit);
    // Media/IDs/typ/booking_date always from API immediately (incl. while contact diffs are open).
    const merged = mergeLookupIntoForm(formHit, hit);
    const next = hitToForm(merged, current);
    if (sink === "edit") setEditForm(next);
    else setForm(next);

    if (diffs.length === 0) {
      setLookupUiForSink(sink, { state: "ok" });
      setLookupAppliedKeyForSink(sink, `${hit.kunden_id}\0${hit.booking_id}`);
      return;
    }
    setLookupUiForSink(sink, { state: "diffs" });
    setDiffApi(hit);
    setDiffList(diffs);
    setDiffResolutions(
      Object.fromEntries(diffs.map((d) => [d.field, "form" as const])),
    );
    setDiffOpen(true);
  }

  function confirmDiff() {
    if (!diffApi) return;
    const sink = lookupSinkRef.current;
    const current = sink === "edit" ? editFormRef.current : formRef.current;
    const formHit = formToLookupHit(current);
    const merged = applyDiffResolutions(formHit, diffApi, diffResolutions);
    const next = hitToForm(merged, current);
    if (sink === "edit") setEditForm(next);
    else setForm(next);
    setLookupUiForSink(sink, { state: "ok" });
    setLookupAppliedKeyForSink(sink, `${diffApi.kunden_id}\0${diffApi.booking_id}`);
    setDiffOpen(false);
    setDiffApi(null);
    setDiffList([]);
  }

  function runIntakeLookup(
    sink: "intake" | "edit",
    kundenId: string,
    bookingId: string,
    appliedKey: string,
  ) {
    const key = `${kundenId}\0${bookingId}`;

    if (!isLookupIdPairReady(kundenId, bookingId)) {
      // Cancel pending / in-flight lookup while the pair is incomplete.
      lookupRequestRef.current += 1;
      setLookupBusyForSink(sink, false);
      setLookupUiForSink(sink, { state: "idle" });
      return () => {};
    }
    if (key === appliedKey) {
      setLookupBusyForSink(sink, false);
      return () => {};
    }

    lookupSinkRef.current = sink;
    const requestId = ++lookupRequestRef.current;
    // Debounce: do not show spinner / searching until the timer fires.
    setLookupBusyForSink(sink, false);
    setLookupUiForSink(sink, { state: "idle" });

    const timer = window.setTimeout(() => {
      if (lookupRequestRef.current !== requestId) return;
      setLookupBusyForSink(sink, true);
      setLookupUiForSink(sink, { state: "searching" });
      void (async () => {
        try {
          const result = await lookupCustomerIntake(kundenId, bookingId);
          if (lookupRequestRef.current !== requestId) return;
          if (result.kind === "hit") {
            applyHitToForm(result.customer, sink);
          } else if (result.kind === "choice") {
            setChoiceHandcam(result.handcam);
            setChoiceOutside(result.outside);
            setChoiceOpen(true);
            setLookupUiForSink(sink, { state: "choice" });
          } else if (result.kind === "not_found") {
            setLookupUiForSink(sink, { state: "not_found" });
            setLookupAppliedKeyForSink(sink, key);
          } else {
            setLookupUiForSink(sink, {
              state: "error",
              message: result.message || "Lookup fehlgeschlagen.",
            });
            showAppToast(result.message || "Customer-Lookup fehlgeschlagen.", {
              tone: "error",
              title: "Lookup",
            });
            setLookupAppliedKeyForSink(sink, key);
          }
        } catch (err) {
          if (lookupRequestRef.current !== requestId) return;
          const message = String(err);
          setLookupUiForSink(sink, { state: "error", message });
          showAppToast(message, { tone: "error", title: "Lookup" });
          setLookupAppliedKeyForSink(sink, key);
        } finally {
          if (lookupRequestRef.current === requestId) {
            setLookupBusyForSink(sink, false);
          }
        }
      })();
    }, LOOKUP_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      if (lookupRequestRef.current === requestId) {
        lookupRequestRef.current += 1;
        setLookupBusyForSink(sink, false);
      }
    };
  }

  useEffect(() => {
    if (!intakeOpen) return;
    return runIntakeLookup(
      "intake",
      form.kunden_id.trim(),
      form.booking_id.trim(),
      lookupAppliedKey,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [intakeOpen, form.kunden_id, form.booking_id, lookupAppliedKey]);

  useEffect(() => {
    if (!editing) return;
    return runIntakeLookup(
      "edit",
      editForm.kunden_id.trim(),
      editForm.booking_id.trim(),
      editLookupAppliedKey,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing, editForm.kunden_id, editForm.booking_id, editLookupAppliedKey]);

  useEffect(() => {
    if (!clipboardCustomer) {
      setClipboardExisting(null);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const hit = await findExistingCustomer(clipboardCustomer);
        if (!cancelled) setClipboardExisting(hit);
      })();
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [clipboardCustomer, items]);

  useEffect(() => {
    if (!intakeOpen) {
      setIntakeExisting(null);
      return;
    }
    const email = form.email.trim();
    const kid = form.kunden_id.trim();
    const bid = form.booking_id.trim();
    if (!email && !(kid && bid)) {
      setIntakeExisting(null);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const hit = await findExistingCustomer({
          email,
          kunden_id: kid,
          booking_id: bid,
        });
        if (!cancelled) setIntakeExisting(hit);
      })();
    }, 300);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    intakeOpen,
    form.email,
    form.kunden_id,
    form.booking_id,
    items,
  ]);

  function applyClipboard(parsed: ClipboardCustomer) {
    setForm((f) => ({
      ...f,
      vorname: parsed.vorname,
      nachname: parsed.nachname,
      email: parsed.email,
      telefon: parsed.telefon,
      ...(parsed.kunden_id ? { kunden_id: parsed.kunden_id } : {}),
      ...(parsed.booking_id ? { booking_id: parsed.booking_id } : {}),
    }));
    if (parsed.kunden_id || parsed.booking_id) {
      setLookupAppliedKey("");
    }
    // Keep clipboard panel until the customer is actually saved.
    setFormError("");
  }

  function resetIntakeDialog() {
    setIntakeOpen(false);
    setForm(EMPTY_FORM);
    setFormError("");
    setAssignAfterSave(false);
    setIntakeSavedCount(0);
    setLookupBusy(false);
    setLookupUi({ state: "idle" });
    setLookupAppliedKey("");
    setDiffOpen(false);
    setDiffApi(null);
    setChoiceOpen(false);
    // Keep lastApplied so the same clipboard payload does not re-flash the banner.
    void checkClipboard();
  }

  function openIntake(prefill?: ClipboardCustomer) {
    setFormError("");
    setAssignAfterSave(false);
    setIntakeSavedCount(0);
    if (prefill) {
      applyClipboard(prefill);
    } else {
      setForm(EMPTY_FORM);
    }
    setIntakeOpen(true);
  }

  function closeIntake() {
    if (formBusy) return;
    resetIntakeDialog();
  }

  async function submitIntake(parsed?: ClipboardCustomer) {
    const payload: FormState = parsed
      ? {
          ...form,
          vorname: parsed.vorname,
          nachname: parsed.nachname,
          email: parsed.email,
          telefon: parsed.telefon,
          ...(parsed.kunden_id ? { kunden_id: parsed.kunden_id } : {}),
          ...(parsed.booking_id ? { booking_id: parsed.booking_id } : {}),
        }
      : form;
    const err = validateForm(payload);
    if (err) {
      setFormError(err);
      if (parsed) {
        applyClipboard(parsed);
        setIntakeOpen(true);
      }
      return null;
    }
    setFormBusy(true);
    setFormError("");
    try {
      const existing =
        intakeExisting &&
        isSameCustomerIdentity(payload, intakeExisting)
          ? intakeExisting
          : await findExistingCustomer(payload);
      if (existing) {
        showAppToast(existingCustomerWarningLabel(existing) + " — wird trotzdem angelegt.", {
          tone: "warning",
          title: "Hinweis",
        });
      }
      return await add(draftFromForm(payload));
    } catch {
      return null;
    } finally {
      setFormBusy(false);
    }
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const shouldAssign = assignAfterSave;
    const customer = await submitIntake();
    if (!customer) return;

    // Suppress re-offer of the just-saved clipboard payload.
    lastAppliedClipboardRef.current = clipboardFingerprint({
      vorname: customer.vorname,
      nachname: customer.nachname,
      email: customer.email,
      telefon: customer.telefon ?? "",
      kunden_id: customer.kunden_id ?? "",
      booking_id: customer.booking_id ?? "",
    });
    setClipboardCustomer(null);
    setClipboardExisting(null);
    setIntakeExisting(null);
    setForm(EMPTY_FORM);
    if (shouldAssign) {
      resetIntakeDialog();
      startAssign(customer);
      return;
    }

    setIntakeSavedCount((n) => n + 1);
    setIntakeFocusKey((n) => n + 1);
    void checkClipboard();
  }

  function onFormPaste(e: ClipboardEvent) {
    const text = e.clipboardData.getData("text");
    const parsed = parseCustomerJsonPaste(text);
    if (!parsed) return;
    e.preventDefault();
    applyClipboard(parsed);
  }

  async function onQuickAddFromClipboard() {
    if (!clipboardCustomer) return;
    const snapshot = clipboardCustomer;
    const customer = await submitIntake(snapshot);
    if (!customer) return;
    lastAppliedClipboardRef.current = clipboardFingerprint(snapshot);
    setClipboardCustomer(null);
    setClipboardExisting(null);
  }

  function openEdit(customer: Customer) {
    setEditing(customer);
    const next = customerToForm(customer);
    setEditForm(next);
    setEditError("");
    setEditLookupUi({ state: "idle" });
    setEditLookupBusy(false);
    // Suppress immediate lookup for the already-stored ID pair; re-run when IDs change.
    setEditLookupAppliedKey(
      isLookupIdPairReady(next.kunden_id, next.booking_id)
        ? `${next.kunden_id.trim()}\0${next.booking_id.trim()}`
        : "",
    );
  }

  async function saveEdit() {
    if (!editing) return;
    const err = validateForm(editForm);
    if (err) {
      setEditError(err);
      return;
    }
    setEditBusy(true);
    try {
      await update({
        ...editing,
        ...draftFromForm(editForm),
      });
      setEditing(null);
    } catch {
      /* store */
    } finally {
      setEditBusy(false);
    }
  }

  async function deleteEdit() {
    if (!editing) return;
    const ok = await confirm("Diesen Kunden wirklich löschen?", {
      title: "Kunde löschen",
      primaryLabel: "Löschen",
      destructive: true,
    });
    if (!ok) return;
    setEditBusy(true);
    try {
      await remove(editing.id);
      setEditing(null);
    } catch {
      /* store */
    } finally {
      setEditBusy(false);
    }
  }

  async function toggleProcessed(customer: Customer) {
    const next = !customer.processed;
    const ok = await confirm(
      next
        ? `${customerLabel(customer)} als erledigt markieren, ohne Ordner zuzuweisen?`
        : `${customerLabel(customer)} wieder in die offene Warteschlange legen?`,
      {
        title: next ? "Als erledigt markieren" : "Wieder öffnen",
        primaryLabel: next ? "Als erledigt" : "Wieder öffnen",
      },
    );
    if (!ok) return;
    await setProcessed(customer.id, next);
  }

  const filters: Array<[CustomerFilter, string]> = [
    ["all", "Alle"],
    ["unprocessed", "Offen"],
    ["processed", "Erledigt"],
  ];

  const emptyQueueText = loading
    ? "Laden…"
    : search.trim()
      ? "Keine Treffer für die Suche."
      : filter === "processed"
        ? "Noch keine erledigten Kunden."
        : filter === "all"
          ? "Noch keine Kunden."
          : "Keine offenen Kunden.";

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-x-3 gap-y-2 border-b border-border/70 bg-card-elevated/40 px-4 py-3">
        <Tabs
          value={view}
          onValueChange={(value) => {
            if (value === "queue" || value === "history") setView(value);
          }}
        >
          <TabsList className="h-8" aria-label="Kundenansicht">
            <TabsTrigger value="queue" className="h-6 gap-1.5 px-2.5 text-xs">
              Warteschlange
              {openCount > 0 ? (
                <span className="rounded-full bg-primary-soft px-1.5 text-[10px] font-semibold leading-4 text-primary">
                  {openCount}
                </span>
              ) : null}
            </TabsTrigger>
            <TabsTrigger value="history" className="h-6 gap-1.5 px-2.5 text-xs">
              Zuweisungen
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={openCount === 0}
            onClick={() => setBatchOpen(true)}
            title="Offene Kunden passenden Ordnern zuordnen"
          >
            <ListChecks className="h-3.5 w-3.5" />
            Passende zuweisen…
          </Button>
          <Button type="button" size="sm" onClick={() => openIntake()}>
            <UserPlus className="h-3.5 w-3.5" />
            Aufnehmen…
          </Button>
        </div>
      </div>

      {view === "queue" && clipboardCustomer && !intakeOpen ? (
        <div
          className={cn(
            "shrink-0 border-b px-4 py-2.5",
            clipboardExisting
              ? "border-amber-500/30 bg-amber-500/10"
              : "border-sky-500/25 bg-sky-500/10",
          )}
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="min-w-0">
              <p className="text-sm text-foreground">
                Zwischenablage:{" "}
                <span className="font-medium">
                  {clipboardCustomer.vorname} {clipboardCustomer.nachname}
                </span>
                <span className="ml-1.5 text-xs text-muted">{clipboardCustomer.email}</span>
              </p>
              {clipboardExisting ? (
                <p className="mt-0.5 flex items-center gap-1 text-xs text-amber-700 dark:text-amber-400">
                  <AlertTriangle className="h-3 w-3 shrink-0" />
                  {existingCustomerWarningLabel(clipboardExisting)}
                </p>
              ) : null}
            </div>
            <div className="flex gap-1.5">
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => openIntake(clipboardCustomer)}
              >
                <ClipboardPaste className="h-3.5 w-3.5" />
                Übernehmen…
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                disabled={formBusy}
                onClick={() => void onQuickAddFromClipboard()}
              >
                Direkt anlegen
              </Button>
            </div>
          </div>
        </div>
      ) : null}

      {view === "queue" ? (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-4 py-3">
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <div className="relative min-w-[12rem] flex-1">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <Input
                className="h-8 pl-8 text-xs"
                placeholder="Suchen…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                aria-label="Kunden durchsuchen"
              />
            </div>
            <div
              className="flex items-center gap-0.5 rounded-lg border border-border bg-card-elevated p-0.5"
              role="group"
              aria-label="Status filtern"
            >
              {filters.map(([key, label]) => {
                const selected = filter === key;
                return (
                  <button
                    key={key}
                    type="button"
                    aria-pressed={selected}
                    className={cn(
                      "inline-flex h-7 items-center gap-1 rounded-md px-2.5 text-xs font-medium transition-colors",
                      "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      selected
                        ? "bg-card text-foreground shadow-sm ring-1 ring-border"
                        : "text-muted hover:text-foreground",
                    )}
                    onClick={() => setFilter(key)}
                  >
                    {label}
                    {key === "unprocessed" && openCount > 0 ? (
                      <span
                        className={cn(
                          "rounded-full px-1.5 text-[10px] font-semibold leading-4",
                          selected
                            ? "bg-primary-soft text-primary"
                            : "bg-background/80 text-muted",
                        )}
                      >
                        {openCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border">
            {items.length === 0 ? (
              <div className="flex h-full min-h-[12rem] flex-col items-center justify-center gap-3 p-6 text-center">
                <p className="text-sm text-muted">{emptyQueueText}</p>
                {!loading && !search.trim() && filter !== "processed" ? (
                  <Button type="button" onClick={() => openIntake()}>
                    <UserPlus className="h-3.5 w-3.5" />
                    Aufnehmen…
                  </Button>
                ) : null}
              </div>
            ) : (
              <ul className="divide-y divide-border">
                {items.map((customer) => {
                  const highlighted = customer.id === highlightId;
                  return (
                    <li
                      key={customer.id}
                      className={cn(
                        "flex flex-wrap items-center gap-2 px-3 py-2.5 transition-colors sm:flex-nowrap",
                        highlighted
                          ? "bg-[var(--ams-row-active)]"
                          : "hover:bg-[var(--ams-row-hover)]",
                      )}
                    >
                      <div className="min-w-0 flex-1 basis-[11rem]">
                        <p className="truncate text-sm font-medium text-foreground">
                          {customerLabel(customer)}
                          {customerHasApiIds(customer) ? (
                            <span className="ml-2 inline-flex items-center rounded-full border border-sky-500/35 bg-sky-500/10 px-1.5 py-px text-[10px] font-medium tracking-wide text-sky-700 uppercase dark:text-sky-300">
                              ID
                            </span>
                          ) : null}
                          {customer.processed ? (
                            <span className="ml-2 inline-flex items-center rounded-full border border-success/30 bg-success/10 px-1.5 py-px text-[10px] font-medium tracking-wide text-success uppercase">
                              erledigt
                            </span>
                          ) : (
                            <span className="ml-2 inline-flex items-center rounded-full border border-warning/35 bg-warning/10 px-1.5 py-px text-[10px] font-medium tracking-wide text-warning uppercase">
                              offen
                            </span>
                          )}
                        </p>
                        <p className="truncate text-xs text-muted">
                          {customer.email}
                          {customer.telefon ? ` · ${customer.telefon}` : ""}
                          {customerHasApiIds(customer)
                            ? ` · #${customer.kunden_id}/${customer.booking_id}`
                            : ""}
                        </p>
                      </div>
                      {customer.assigned_path.trim() ? (
                        <div className="min-w-0 flex-1 basis-[12rem]">
                          <p className="truncate text-xs text-muted">
                            {formatHistoryDate(
                              history.find((entry) => entry.customer_id === customer.id)
                                ?.created_at || customer.updated_at,
                            )}
                          </p>
                          <p
                            className="truncate font-mono text-xs text-foreground"
                            title={customer.assigned_path}
                          >
                            {assignedDirName(customer.assigned_path)}
                          </p>
                        </div>
                      ) : null}
                      <div className="flex shrink-0 flex-wrap items-center gap-1">
                        <Button
                          type="button"
                          size="sm"
                          variant={customer.processed ? "secondary" : "default"}
                          onClick={() => startAssign(customer)}
                          title={
                            customer.processed
                              ? "Erneut einem Ordner zuweisen"
                              : "Ordner wählen"
                          }
                        >
                          <FolderInput className="h-3.5 w-3.5" />
                          {customer.processed ? "Erneut zuweisen" : "Zuweisen"}
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => openEdit(customer)}
                          title="Bearbeiten"
                          aria-label="Bearbeiten"
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => void toggleProcessed(customer)}
                          title={
                            customer.processed
                              ? "Als offen markieren"
                              : "Als erledigt markieren"
                          }
                          aria-label={
                            customer.processed
                              ? "Als offen markieren"
                              : "Als erledigt markieren"
                          }
                        >
                          {customer.processed ? (
                            <RotateCcw className="h-3.5 w-3.5" />
                          ) : (
                            <Check className="h-3.5 w-3.5" />
                          )}
                        </Button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {history.length === 0 ? (
            <div className="flex min-h-[12rem] flex-col items-center justify-center gap-3 text-center">
              <p className="text-sm text-muted">Noch keine Zuweisungen.</p>
              <Button type="button" variant="secondary" onClick={() => setView("queue")}>
                Zur Warteschlange
              </Button>
            </div>
          ) : (
            <div className="overflow-x-auto rounded-lg border border-border">
              <table className="w-full min-w-[32rem] text-left text-sm">
                <thead className="border-b border-border bg-card-elevated text-xs text-muted">
                  <tr>
                    <th className="px-3 py-2 font-medium">Zeit</th>
                    <th className="px-3 py-2 font-medium">Kunde</th>
                    <th className="px-3 py-2 font-medium">E-Mail</th>
                    <th className="px-3 py-2 font-medium">Pfad</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {history.map((entry) => (
                    <tr key={entry.id} className="hover:bg-[var(--ams-row-hover)]">
                      <td className="px-3 py-2 whitespace-nowrap text-xs text-muted">
                        {formatHistoryDate(entry.created_at)}
                      </td>
                      <td className="px-3 py-2">
                        {entry.vorname} {entry.nachname}
                      </td>
                      <td className="px-3 py-2 text-muted">{entry.email}</td>
                      <td
                        className="max-w-[16rem] truncate px-3 py-2 font-mono text-xs text-muted"
                        title={entry.file_path}
                      >
                        {entry.file_path}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      <Dialog
        open={intakeOpen}
        onOpenChange={(open) => {
          if (!open) closeIntake();
        }}
      >
        <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Kunde aufnehmen</DialogTitle>
            <DialogDescription>
              Formular bleibt offen für den nächsten Kunden.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => void onSubmit(e)} onPaste={onFormPaste} className="space-y-2.5">
            {clipboardCustomer ? (
              <div
                className={cn(
                  "rounded-lg border px-3 py-2.5",
                  clipboardExisting || intakeExisting
                    ? "border-amber-500/30 bg-amber-500/10"
                    : "border-sky-500/30 bg-sky-500/10",
                )}
              >
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm text-foreground">
                    Zwischenablage:{" "}
                    <span className="font-medium">
                      {clipboardCustomer.vorname} {clipboardCustomer.nachname}
                    </span>
                  </p>
                  {formMatchesClipboard(form, clipboardCustomer) ? (
                    <span className="shrink-0 text-xs text-muted">im Formular</span>
                  ) : (
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      onClick={() => applyClipboard(clipboardCustomer)}
                    >
                      <ClipboardPaste className="h-3.5 w-3.5" />
                      Einfügen
                    </Button>
                  )}
                </div>
                {(clipboardExisting || intakeExisting) ? (
                  <p className="mt-1.5 flex items-center gap-1 text-xs text-amber-700 dark:text-amber-400">
                    <AlertTriangle className="h-3 w-3 shrink-0" />
                    {existingCustomerWarningLabel(clipboardExisting ?? intakeExisting!)}
                  </p>
                ) : null}
              </div>
            ) : intakeExisting ? (
              <p className="flex items-center gap-1.5 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400">
                <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                {existingCustomerWarningLabel(intakeExisting)} — Speichern trotzdem möglich.
              </p>
            ) : null}

            <div className="grid gap-2.5 sm:grid-cols-2">
              <Field
                inputRef={kundenIdRef}
                label="Kunden-ID (optional)"
                value={form.kunden_id}
                onChange={(v) => {
                  setLookupAppliedKey("");
                  setForm((f) => ({ ...f, kunden_id: sanitizeNumericIdInput(v) }));
                }}
                inputMode="numeric"
                mono
                prefix={<Hash className="h-3.5 w-3.5 text-muted" />}
                suffix={
                  lookupBusy ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
                  ) : undefined
                }
              />
              <Field
                label="Buchungs-ID (optional)"
                value={form.booking_id}
                onChange={(v) => {
                  setLookupAppliedKey("");
                  setForm((f) => ({ ...f, booking_id: sanitizeNumericIdInput(v) }));
                }}
                inputMode="numeric"
                mono
                prefix={<Hash className="h-3.5 w-3.5 text-muted" />}
                suffix={
                  lookupBusy ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
                  ) : undefined
                }
              />
            </div>
            <LookupStatusBanner ui={lookupUi} />

            <div className="grid gap-2.5 sm:grid-cols-2">
              <Field
                label="Vorname *"
                value={form.vorname}
                onChange={(v) => setForm((f) => ({ ...f, vorname: v }))}
              />
              <Field
                label="Nachname *"
                value={form.nachname}
                onChange={(v) => setForm((f) => ({ ...f, nachname: v }))}
              />
            </div>
            <div className="grid gap-2.5 sm:grid-cols-2">
              <Field
                label="E-Mail *"
                type="email"
                value={form.email}
                onChange={(v) => setForm((f) => ({ ...f, email: v }))}
              />
              <Field
                label="Telefon (optional)"
                type="tel"
                value={form.telefon}
                onChange={(v) => setForm((f) => ({ ...f, telefon: v }))}
              />
            </div>
            <MediaFlagsSwitches
              values={form}
              onChange={(key, on) =>
                setForm((f) => ({
                  ...f,
                  [key]: on,
                  [`ist_bezahlt_${key}`]: on,
                }))
              }
            />
            <div className="grid gap-2.5 sm:grid-cols-2">
              <BookingDateField
                value={form.booking_date}
                onChange={(v) => setForm((f) => ({ ...f, booking_date: v }))}
              />
              <div className="space-y-1.5">
                <Label htmlFor="assign-after-save">Direkt zuweisen</Label>
                <div className="flex h-9 items-center">
                  <Switch
                    id="assign-after-save"
                    checked={assignAfterSave}
                    onCheckedChange={(v) => setAssignAfterSave(v === true)}
                    aria-label="Direkt zuweisen"
                  />
                </div>
              </div>
            </div>

            {formError ? <p className="text-xs text-destructive">{formError}</p> : null}

            <DialogFooter className="gap-2 pt-1">
              <Button type="button" variant="secondary" disabled={formBusy} onClick={closeIntake}>
                {intakeSavedCount > 0 ? "Fertig" : "Abbrechen"}
              </Button>
              <Button type="submit" disabled={formBusy}>
                <UserPlus className="h-3.5 w-3.5" />
                {formBusy
                  ? assignAfterSave
                    ? "Zuweisen…"
                    : "Speichern…"
                  : assignAfterSave
                    ? "Speichern & Zuweisen"
                    : "In Warteschlange"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <FolderSelectionModal
        open={Boolean(exportingId)}
        busy={assignBusy}
        customerLabel={exportingLabel || undefined}
        vorname={exportingVorname}
        nachname={exportingNachname}
        email={exportingEmail}
        onClose={() => {
          if (!assignBusy) {
            clearExporting();
          }
        }}
        onSelect={(path) => void onAssign(path)}
      />

      <IdAssignReviewDialog
        open={reviewOpen}
        initial={reviewPreview}
        busy={assignBusy}
        onCancel={() => {
          if (assignBusy) return;
          setReviewOpen(false);
          setReviewPreview(null);
          setReviewFolderPath("");
          clearExporting();
        }}
        onConfirm={(override) => {
          if (!reviewPreview || !reviewFolderPath) return;
          void runAssign(reviewPreview.customer_id, reviewFolderPath, override);
        }}
      />

      <BatchAssignDialog open={batchOpen} onClose={() => setBatchOpen(false)} />

      <Dialog
        open={Boolean(editing)}
        onOpenChange={(v) => {
          if (!v && !editBusy) {
            setEditing(null);
            setEditLookupUi({ state: "idle" });
            setEditLookupBusy(false);
            setEditLookupAppliedKey("");
          }
        }}
      >
        <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Kunde bearbeiten</DialogTitle>
          </DialogHeader>
          <div className="space-y-2.5">
            <div className="grid gap-2.5 sm:grid-cols-2">
              <Field
                label="Kunden-ID"
                value={editForm.kunden_id}
                onChange={(v) => {
                  setEditLookupAppliedKey("");
                  setEditForm((f) => ({ ...f, kunden_id: sanitizeNumericIdInput(v) }));
                }}
                inputMode="numeric"
                mono
                prefix={<Hash className="h-3.5 w-3.5 text-muted" />}
                suffix={
                  editLookupBusy ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
                  ) : undefined
                }
              />
              <Field
                label="Buchungs-ID"
                value={editForm.booking_id}
                onChange={(v) => {
                  setEditLookupAppliedKey("");
                  setEditForm((f) => ({ ...f, booking_id: sanitizeNumericIdInput(v) }));
                }}
                inputMode="numeric"
                mono
                prefix={<Hash className="h-3.5 w-3.5 text-muted" />}
                suffix={
                  editLookupBusy ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
                  ) : undefined
                }
              />
            </div>
            <LookupStatusBanner ui={editLookupUi} />
            <div className="grid gap-2.5 sm:grid-cols-2">
              <Field
                label="Vorname *"
                value={editForm.vorname}
                onChange={(v) => setEditForm((f) => ({ ...f, vorname: v }))}
              />
              <Field
                label="Nachname *"
                value={editForm.nachname}
                onChange={(v) => setEditForm((f) => ({ ...f, nachname: v }))}
              />
            </div>
            <div className="grid gap-2.5 sm:grid-cols-2">
              <Field
                label="E-Mail *"
                type="email"
                value={editForm.email}
                onChange={(v) => setEditForm((f) => ({ ...f, email: v }))}
              />
              <Field
                label="Telefon"
                type="tel"
                value={editForm.telefon}
                onChange={(v) => setEditForm((f) => ({ ...f, telefon: v }))}
              />
            </div>
            <MediaFlagsSwitches
              values={editForm}
              onChange={(key, on) =>
                setEditForm((f) => ({
                  ...f,
                  [key]: on,
                  [`ist_bezahlt_${key}`]: on,
                }))
              }
            />
            <div className="grid gap-2.5 sm:grid-cols-2">
              <BookingDateField
                value={editForm.booking_date}
                onChange={(v) => setEditForm((f) => ({ ...f, booking_date: v }))}
              />
            </div>
            {editError ? <p className="text-xs text-destructive">{editError}</p> : null}
          </div>
          <DialogFooter className="gap-2 sm:justify-between">
            <Button
              type="button"
              variant="destructive"
              disabled={editBusy}
              onClick={() => void deleteEdit()}
            >
              <Trash2 className="h-3.5 w-3.5" />
              Löschen
            </Button>
            <div className="flex gap-2">
              <Button
                type="button"
                variant="secondary"
                disabled={editBusy}
                onClick={() => setEditing(null)}
              >
                Abbrechen
              </Button>
              <Button type="button" disabled={editBusy} onClick={() => void saveEdit()}>
                Speichern
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <CustomerLookupDiffDialog
        open={diffOpen}
        diffs={diffList}
        resolutions={diffResolutions}
        onResolve={(field, choice) =>
          setDiffResolutions((prev) => ({ ...prev, [field]: choice }))
        }
        onApplyAllApi={() => {
          if (!diffApi) return;
          const sink = lookupSinkRef.current;
          const current = sink === "edit" ? editFormRef.current : formRef.current;
          const next = hitToForm(applyAllApi(formToLookupHit(current), diffApi), current);
          if (sink === "edit") setEditForm(next);
          else setForm(next);
          setLookupAppliedKeyForSink(sink, `${diffApi.kunden_id}\0${diffApi.booking_id}`);
          setLookupUiForSink(sink, { state: "ok" });
          setDiffOpen(false);
          setDiffApi(null);
        }}
        onKeepForm={() => {
          if (!diffApi) return;
          const sink = lookupSinkRef.current;
          const current = sink === "edit" ? editFormRef.current : formRef.current;
          const merged = mergeLookupIntoForm(formToLookupHit(current), diffApi);
          const next = hitToForm(merged, current);
          if (sink === "edit") setEditForm(next);
          else setForm(next);
          setLookupAppliedKeyForSink(sink, `${diffApi.kunden_id}\0${diffApi.booking_id}`);
          setLookupUiForSink(sink, { state: "ok" });
          setDiffOpen(false);
          setDiffApi(null);
        }}
        onConfirm={confirmDiff}
      />

      {choiceHandcam && choiceOutside ? (
        <CustomerLookupChoiceDialog
          open={choiceOpen}
          handcam={choiceHandcam}
          outside={choiceOutside}
          onPick={(hit) => {
            const sink = lookupSinkRef.current;
            setChoiceOpen(false);
            setChoiceHandcam(null);
            setChoiceOutside(null);
            applyHitToForm(hit, sink);
            setLookupBusyForSink(sink, false);
          }}
          onCancel={() => {
            const sink = lookupSinkRef.current;
            const current = sink === "edit" ? editFormRef.current : formRef.current;
            setChoiceOpen(false);
            setChoiceHandcam(null);
            setChoiceOutside(null);
            setLookupBusyForSink(sink, false);
            setLookupUiForSink(sink, { state: "cancelled" });
            setLookupAppliedKeyForSink(
              sink,
              `${current.kunden_id.trim()}\0${current.booking_id.trim()}`,
            );
          }}
        />
      ) : null}
    </div>
  );
}

function BookingDateField({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const iso = toStorageBookingDate(value);
  const dateValue = /^\d{4}-\d{2}-\d{2}$/.test(iso) ? iso : "";
  return (
    <div className="space-y-1.5">
      <Label>Buchungsdatum</Label>
      <Input
        type="date"
        value={dateValue}
        onChange={(e) => onChange(toDisplayBookingDate(e.target.value))}
        className={cn(
          "relative pr-9",
          "[&::-webkit-calendar-picker-indicator]:absolute",
          "[&::-webkit-calendar-picker-indicator]:right-2.5",
          "[&::-webkit-calendar-picker-indicator]:top-1/2",
          "[&::-webkit-calendar-picker-indicator]:-translate-y-1/2",
          "[&::-webkit-calendar-picker-indicator]:cursor-pointer",
        )}
      />
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  type = "text",
  inputRef,
  inputMode,
  mono,
  hint,
  placeholder,
  prefix,
  suffix,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  inputRef?: Ref<HTMLInputElement>;
  inputMode?: "numeric" | "text" | "email" | "tel";
  mono?: boolean;
  hint?: string;
  placeholder?: string;
  prefix?: ReactNode;
  suffix?: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <div className="relative">
        <Input
          ref={inputRef}
          type={type}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          autoComplete="off"
          inputMode={inputMode}
          placeholder={placeholder}
          className={cn(
            mono && "font-mono",
            prefix && "pl-8",
            suffix && "pr-8",
          )}
        />
        {prefix ? (
          <span className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2">
            {prefix}
          </span>
        ) : null}
        {suffix ? (
          <span className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2">
            {suffix}
          </span>
        ) : null}
      </div>
      {hint ? <p className="text-[11px] text-muted">{hint}</p> : null}
    </div>
  );
}

function LookupStatusBanner({ ui }: { ui: LookupUi }) {
  if (ui.state === "idle") return null;

  if (ui.state === "searching") {
    return (
      <div className="flex items-center gap-2 rounded-md border border-border/70 bg-card-elevated/40 px-2.5 py-1.5 text-xs text-muted">
        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
        Customer-API wird abgefragt…
      </div>
    );
  }

  if (ui.state === "ok") {
    return (
      <div className="flex items-center gap-2 rounded-md border border-success/30 bg-success/10 px-2.5 py-1.5 text-xs text-success">
        <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
        Lookup erfolgreich — Daten übernommen
      </div>
    );
  }

  if (ui.state === "not_found") {
    return (
      <div className="flex items-center gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2.5 py-1.5 text-xs text-amber-700 dark:text-amber-400">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        Kein Treffer für diese IDs
      </div>
    );
  }

  if (ui.state === "error") {
    return (
      <div className="flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
        <XCircle className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 truncate">Lookup fehlgeschlagen{ui.message ? `: ${ui.message}` : ""}</span>
      </div>
    );
  }

  if (ui.state === "choice") {
    return (
      <div className="flex items-center gap-2 rounded-md border border-sky-500/30 bg-sky-500/10 px-2.5 py-1.5 text-xs text-foreground">
        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
        Medientyp wählen…
      </div>
    );
  }

  if (ui.state === "diffs") {
    return (
      <div className="flex items-center gap-2 rounded-md border border-sky-500/30 bg-sky-500/10 px-2.5 py-1.5 text-xs text-foreground">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        Kontaktdaten weichen ab — bitte Diff prüfen
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2 rounded-md border border-border/70 bg-card-elevated/40 px-2.5 py-1.5 text-xs text-muted">
      Lookup abgebrochen
    </div>
  );
}

type MediaFlagKey =
  | "handcam_foto"
  | "handcam_video"
  | "outside_foto"
  | "outside_video";

const MEDIA_FLAG_LAYOUT: Array<[MediaFlagKey, string]> = [
  ["handcam_foto", "Handcam Foto"],
  ["handcam_video", "Handcam Video"],
  ["outside_foto", "Outside Foto"],
  ["outside_video", "Outside Video"],
];

function MediaFlagsSwitches({
  values,
  onChange,
}: {
  values: Pick<FormState, MediaFlagKey>;
  onChange: (key: MediaFlagKey, on: boolean) => void;
}) {
  return (
    <div className="space-y-2 rounded-md border border-border/70 bg-card-elevated/40 px-2.5 py-2">
      <Label className="text-xs text-muted">Medienoptionen</Label>
      <div className="grid grid-cols-2 gap-x-0">
        <div className="space-y-2 border-r-2 border-border pr-5">
          {MEDIA_FLAG_LAYOUT.filter(([key]) => key.startsWith("handcam_")).map(
            ([key, label]) => (
              <label
                key={key}
                className="flex items-center justify-between gap-2 text-xs text-foreground"
              >
                <span className="min-w-0 truncate">{label}</span>
                <Switch
                  checked={values[key]}
                  onCheckedChange={(v) => onChange(key, v === true)}
                  aria-label={label}
                />
              </label>
            ),
          )}
        </div>
        <div className="space-y-2 pl-5">
          {MEDIA_FLAG_LAYOUT.filter(([key]) => key.startsWith("outside_")).map(
            ([key, label]) => (
              <label
                key={key}
                className="flex items-center justify-between gap-2 text-xs text-foreground"
              >
                <span className="min-w-0 truncate">{label}</span>
                <Switch
                  checked={values[key]}
                  onCheckedChange={(v) => onChange(key, v === true)}
                  aria-label={label}
                />
              </label>
            ),
          )}
        </div>
      </div>
    </div>
  );
}
