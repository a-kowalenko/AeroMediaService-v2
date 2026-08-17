import { create } from "zustand";

export type WorkspaceTab = "history" | "customers";
export type DialogKind = "error" | "success" | "warning" | "confirm" | "prompt" | null;

const WORKSPACE_TAB_KEY = "ams.workspaceTab";

function readWorkspaceTab(): WorkspaceTab {
  try {
    const value = sessionStorage.getItem(WORKSPACE_TAB_KEY);
    if (value === "history" || value === "customers") return value;
  } catch {
    /* private mode / unavailable */
  }
  return "history";
}

export type DialogPrimaryAction = {
  label: string;
};

export type ConfirmDialogOptions = {
  title?: string;
  primaryLabel?: string;
  secondaryLabel?: string;
  destructive?: boolean;
};

export type PromptDialogOptions = {
  title?: string;
  primaryLabel?: string;
  secondaryLabel?: string;
  defaultValue?: string;
  placeholder?: string;
  /** Shown above the input (optional hint). */
  hint?: string;
};

type UiState = {
  workspaceTab: WorkspaceTab;
  setWorkspaceTab: (tab: WorkspaceTab) => void;
  dialogKind: DialogKind;
  dialogTitle: string;
  dialogMessage: string;
  dialogAutoCloseSecs: number | null;
  dialogPrimaryAction: DialogPrimaryAction | null;
  dialogPrimaryLabel: string;
  dialogSecondaryLabel: string;
  dialogDestructive: boolean;
  dialogPromptValue: string;
  dialogPromptPlaceholder: string;
  dialogPromptHint: string;
  showError: (
    message: string,
    title?: string,
    options?: { primaryAction?: DialogPrimaryAction },
  ) => void;
  showSuccess: (
    message: string,
    title?: string,
    options?: { autoCloseSecs?: number },
  ) => void;
  showWarning: (
    message: string,
    title?: string,
    options?: { autoCloseSecs?: number },
  ) => void;
  /** Promise resolves true if primary confirmed. */
  confirm: (message: string, options?: ConfirmDialogOptions) => Promise<boolean>;
  /** Promise resolves string on OK, null on cancel. */
  prompt: (message: string, options?: PromptDialogOptions) => Promise<string | null>;
  closeDialog: () => void;
  resolveConfirm: (accepted: boolean) => void;
  resolvePrompt: (value: string | null) => void;
  setPromptValue: (value: string) => void;
};

const emptyDialogFields = {
  dialogTitle: "",
  dialogMessage: "",
  dialogAutoCloseSecs: null as number | null,
  dialogPrimaryAction: null as DialogPrimaryAction | null,
  dialogPrimaryLabel: "OK",
  dialogSecondaryLabel: "Abbrechen",
  dialogDestructive: false,
  dialogPromptValue: "",
  dialogPromptPlaceholder: "",
  dialogPromptHint: "",
};

let confirmResolver: ((value: boolean) => void) | null = null;
let promptResolver: ((value: string | null) => void) | null = null;

function settleConfirm(value: boolean) {
  const resolve = confirmResolver;
  confirmResolver = null;
  resolve?.(value);
}

function settlePrompt(value: string | null) {
  const resolve = promptResolver;
  promptResolver = null;
  resolve?.(value);
}

export const useUiStore = create<UiState>((set, get) => ({
  workspaceTab: readWorkspaceTab(),
  setWorkspaceTab: (workspaceTab) => {
    set({ workspaceTab });
    try {
      sessionStorage.setItem(WORKSPACE_TAB_KEY, workspaceTab);
    } catch {
      /* ignore */
    }
  },
  dialogKind: null,
  ...emptyDialogFields,

  showError: (message, title = "Fehler", options) => {
    settleConfirm(false);
    settlePrompt(null);
    set({
      dialogKind: "error",
      ...emptyDialogFields,
      dialogTitle: title,
      dialogMessage: message,
      dialogPrimaryAction: options?.primaryAction ?? null,
    });
  },

  showSuccess: (message, title = "Erfolg", options) => {
    settleConfirm(false);
    settlePrompt(null);
    set({
      dialogKind: "success",
      ...emptyDialogFields,
      dialogTitle: title,
      dialogMessage: message,
      dialogAutoCloseSecs:
        options?.autoCloseSecs && options.autoCloseSecs > 0
          ? options.autoCloseSecs
          : null,
    });
  },

  showWarning: (message, title = "Hinweis", options) => {
    settleConfirm(false);
    settlePrompt(null);
    set({
      dialogKind: "warning",
      ...emptyDialogFields,
      dialogTitle: title,
      dialogMessage: message,
      dialogAutoCloseSecs:
        options?.autoCloseSecs && options.autoCloseSecs > 0
          ? options.autoCloseSecs
          : null,
    });
  },

  confirm: (message, options) =>
    new Promise<boolean>((resolve) => {
      settleConfirm(false);
      settlePrompt(null);
      confirmResolver = resolve;
      set({
        dialogKind: "confirm",
        ...emptyDialogFields,
        dialogTitle: options?.title ?? "Bestätigen",
        dialogMessage: message,
        dialogPrimaryLabel: options?.primaryLabel ?? "OK",
        dialogSecondaryLabel: options?.secondaryLabel ?? "Abbrechen",
        dialogDestructive: Boolean(options?.destructive),
      });
    }),

  prompt: (message, options) =>
    new Promise<string | null>((resolve) => {
      settleConfirm(false);
      settlePrompt(null);
      promptResolver = resolve;
      set({
        dialogKind: "prompt",
        ...emptyDialogFields,
        dialogTitle: options?.title ?? "Eingabe",
        dialogMessage: message,
        dialogPrimaryLabel: options?.primaryLabel ?? "OK",
        dialogSecondaryLabel: options?.secondaryLabel ?? "Abbrechen",
        dialogPromptValue: options?.defaultValue ?? "",
        dialogPromptPlaceholder: options?.placeholder ?? "",
        dialogPromptHint: options?.hint ?? "",
      });
    }),

  closeDialog: () => {
    const kind = get().dialogKind;
    if (kind === "confirm") settleConfirm(false);
    if (kind === "prompt") settlePrompt(null);
    set({
      dialogKind: null,
      ...emptyDialogFields,
    });
  },

  resolveConfirm: (accepted) => {
    settleConfirm(accepted);
    set({
      dialogKind: null,
      ...emptyDialogFields,
    });
  },

  resolvePrompt: (value) => {
    settlePrompt(value);
    set({
      dialogKind: null,
      ...emptyDialogFields,
    });
  },

  setPromptValue: (value) => set({ dialogPromptValue: value }),
}));
