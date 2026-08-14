import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogPortal = DialogPrimitive.Portal;
export const DialogClose = DialogPrimitive.Close;

/** Prefer explicit z-[n] / z-n from overlay or content so stacked dialogs stay ordered. */
function layerZIndexClass(...classNames: (string | undefined)[]): string {
  for (const c of classNames) {
    if (!c) continue;
    const match = c.match(/(?:^|\s)(z-\[\d+\]|z-(?:\d+|auto|popover|modal|toast))(?:\s|$)/);
    if (match) return match[1];
  }
  return "z-50";
}

export const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    className={cn("fixed inset-0 z-50 bg-black/45 backdrop-blur-[2px] dark:bg-black/60", className)}
    {...props}
  />
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

export const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    hideCloseButton?: boolean;
    overlayClassName?: string;
    /** Classes for the fixed flex wrapper (alignment, chrome insets). */
    containerClassName?: string;
  }
>(({ className, children, hideCloseButton = false, overlayClassName, containerClassName, ...props }, ref) => {
  const layerZ = layerZIndexClass(overlayClassName, className, containerClassName);

  return (
    <DialogPortal>
      <DialogOverlay className={overlayClassName} />
      {/*
        Flex-center the panel without transform on Content, so portaled Select/Combobox
        fixed coords stay correct — and WebKit (macOS) doesn't break on inset+m-auto+h-fit.
      */}
      <div
        className={cn(
          "pointer-events-none fixed inset-0 flex items-center justify-center p-4",
          layerZ,
          containerClassName,
        )}
      >
        <DialogPrimitive.Content
          ref={ref}
          className={cn(
            "pointer-events-auto relative grid w-full min-w-0 max-w-[min(32rem,calc(100vw-2rem))] max-h-[min(90vh,calc(100dvh-2rem))] gap-4 overflow-x-hidden overflow-y-auto rounded-lg border border-border bg-card p-6 shadow-lg",
            className,
          )}
          {...props}
        >
          {children}
          {!hideCloseButton && (
            <DialogPrimitive.Close className="absolute right-4 top-4 rounded-sm opacity-70 transition-opacity hover:opacity-100">
              <X className="h-4 w-4" />
              <span className="sr-only">Schließen</span>
            </DialogPrimitive.Close>
          )}
        </DialogPrimitive.Content>
      </div>
    </DialogPortal>
  );
});
DialogContent.displayName = DialogPrimitive.Content.displayName;

export function DialogHeader({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("flex min-w-0 flex-col space-y-1.5 text-left", className)} {...props} />
  );
}

export function DialogFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "flex min-w-0 flex-col-reverse flex-wrap gap-2 sm:flex-row sm:justify-end",
        className,
      )}
      {...props}
    />
  );
}

export const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("text-lg font-semibold leading-none tracking-tight", className)}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

export const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-muted", className)}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;
