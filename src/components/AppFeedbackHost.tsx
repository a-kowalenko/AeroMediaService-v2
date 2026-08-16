import { ErrorDialog } from "@/components/ErrorDialog";
import { SuccessDialog } from "@/components/SuccessDialog";
import { WarningDialog } from "@/components/WarningDialog";
import { ConfirmDialog, PromptDialog } from "@/components/ConfirmDialog";
import { ToastHost } from "@/components/ToastHost";
import { useUiStore } from "@/store/uiStore";

/** Renders global dialogs + toasts from `useUiStore` (ATS-style feedback). */
export function AppFeedbackHost() {
  const dialogKind = useUiStore((s) => s.dialogKind);
  const dialogTitle = useUiStore((s) => s.dialogTitle);
  const dialogMessage = useUiStore((s) => s.dialogMessage);
  const dialogAutoCloseSecs = useUiStore((s) => s.dialogAutoCloseSecs);
  const dialogPrimaryAction = useUiStore((s) => s.dialogPrimaryAction);
  const dialogPrimaryLabel = useUiStore((s) => s.dialogPrimaryLabel);
  const dialogSecondaryLabel = useUiStore((s) => s.dialogSecondaryLabel);
  const dialogDestructive = useUiStore((s) => s.dialogDestructive);
  const dialogPromptValue = useUiStore((s) => s.dialogPromptValue);
  const dialogPromptPlaceholder = useUiStore((s) => s.dialogPromptPlaceholder);
  const dialogPromptHint = useUiStore((s) => s.dialogPromptHint);
  const closeDialog = useUiStore((s) => s.closeDialog);
  const resolveConfirm = useUiStore((s) => s.resolveConfirm);
  const resolvePrompt = useUiStore((s) => s.resolvePrompt);
  const setPromptValue = useUiStore((s) => s.setPromptValue);

  return (
    <>
      <ErrorDialog
        open={dialogKind === "error"}
        title={dialogTitle}
        message={dialogMessage}
        primaryAction={dialogPrimaryAction}
        onClose={closeDialog}
      />
      <SuccessDialog
        open={dialogKind === "success"}
        title={dialogTitle}
        message={dialogMessage}
        autoCloseSecs={dialogAutoCloseSecs}
        onClose={closeDialog}
      />
      <WarningDialog
        open={dialogKind === "warning"}
        title={dialogTitle}
        message={dialogMessage}
        autoCloseSecs={dialogAutoCloseSecs}
        onClose={closeDialog}
      />
      <ConfirmDialog
        open={dialogKind === "confirm"}
        title={dialogTitle}
        message={dialogMessage}
        primaryLabel={dialogPrimaryLabel}
        secondaryLabel={dialogSecondaryLabel}
        destructive={dialogDestructive}
        onConfirm={() => resolveConfirm(true)}
        onCancel={() => resolveConfirm(false)}
      />
      <PromptDialog
        open={dialogKind === "prompt"}
        title={dialogTitle}
        message={dialogMessage}
        value={dialogPromptValue}
        placeholder={dialogPromptPlaceholder}
        hint={dialogPromptHint}
        primaryLabel={dialogPrimaryLabel}
        secondaryLabel={dialogSecondaryLabel}
        onChange={setPromptValue}
        onConfirm={() => resolvePrompt(dialogPromptValue)}
        onCancel={() => resolvePrompt(null)}
      />
      <ToastHost />
    </>
  );
}
