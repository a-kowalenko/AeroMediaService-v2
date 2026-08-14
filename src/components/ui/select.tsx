import * as React from "react";
import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

/** Match Combobox / DateField overlay stacking (Dialog uses z-50). */
const SELECT_LIST_Z = 80;

/**
 * Radix modal dialogs mark body siblings as inert — a body-portaled list
 * would not receive clicks. Prefer the nearest open-trigger dialog, like Combobox.
 * Select trigger is `button[role=combobox]`; our Combobox uses an input.
 */
function resolveSelectPortalContainer(
  explicit?: HTMLElement | null,
): HTMLElement | undefined {
  if (explicit) return explicit;
  if (typeof document === "undefined") return undefined;
  const trigger = document.querySelector<HTMLElement>(
    'button[role="combobox"][data-state="open"]',
  );
  const dialog = trigger?.closest('[role="dialog"]');
  if (dialog instanceof HTMLElement) return dialog;
  return document.body;
}

export const Select = SelectPrimitive.Root;
export const SelectValue = SelectPrimitive.Value;
export const SelectGroup = SelectPrimitive.Group;

export const SelectTrigger = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Trigger>
>(({ className, children, ...props }, ref) => (
  <SelectPrimitive.Trigger
    ref={ref}
    className={cn(
      "flex h-9 w-full items-center justify-between gap-2 rounded-md border border-border bg-card px-3 py-2 text-sm shadow-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
      "[&[data-state=open]_svg.ats-select-chevron]:rotate-180",
      className,
    )}
    {...props}
  >
    {children}
    <SelectPrimitive.Icon asChild>
      <ChevronDown className="ats-select-chevron h-4 w-4 shrink-0 text-muted transition-transform" />
    </SelectPrimitive.Icon>
  </SelectPrimitive.Trigger>
));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;

type SelectContentProps = React.ComponentPropsWithoutRef<
  typeof SelectPrimitive.Content
> & {
  /** Override portal target (default: nearest dialog or `document.body`). */
  container?: HTMLElement | null;
};

export const SelectContent = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Content>,
  SelectContentProps
>(
  (
    {
      className,
      children,
      position = "popper",
      container,
      sideOffset = 4,
      collisionPadding = 8,
      ...props
    },
    ref,
  ) => {
    const portalContainer = resolveSelectPortalContainer(container);
    const { style, ...rest } = props;

    return (
      <SelectPrimitive.Portal container={portalContainer}>
        <SelectPrimitive.Content
          ref={ref}
          className={cn(
            "relative max-h-60 min-w-[8rem] overflow-hidden rounded-md border border-border bg-card text-foreground shadow-md",
            position === "popper" &&
              "data-[side=bottom]:translate-y-0 data-[side=top]:translate-y-0",
            className,
          )}
          position={position}
          sideOffset={sideOffset}
          collisionPadding={collisionPadding}
          {...rest}
          style={{ zIndex: SELECT_LIST_Z, ...style }}
        >
          <SelectPrimitive.Viewport
            className={cn(
              "py-1",
              position === "popper" &&
                "w-full min-w-[var(--radix-select-trigger-width)]",
            )}
          >
            {children}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    );
  },
);
SelectContent.displayName = SelectPrimitive.Content.displayName;

export const SelectLabel = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Label>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Label>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.Label
    ref={ref}
    className={cn("px-3 py-1.5 text-xs font-medium text-muted", className)}
    {...props}
  />
));
SelectLabel.displayName = SelectPrimitive.Label.displayName;

export const SelectItem = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Item>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Item>
>(({ className, children, ...props }, ref) => (
  <SelectPrimitive.Item
    ref={ref}
    className={cn(
      "relative flex w-full cursor-default select-none items-center py-1.5 pr-3 pl-8 text-sm text-foreground outline-none",
      "data-[highlighted]:bg-primary-soft data-[highlighted]:text-foreground",
      "data-[state=checked]:text-primary",
      "data-[disabled]:pointer-events-none data-[disabled]:cursor-not-allowed data-[disabled]:text-muted/50 data-[disabled]:opacity-100",
      className,
    )}
    {...props}
  >
    <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
      <SelectPrimitive.ItemIndicator>
        <Check className="h-4 w-4 text-primary" />
      </SelectPrimitive.ItemIndicator>
    </span>
    <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
  </SelectPrimitive.Item>
));
SelectItem.displayName = SelectPrimitive.Item.displayName;

export const SelectSeparator = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Separator>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Separator>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.Separator
    ref={ref}
    className={cn("my-1 h-px bg-border", className)}
    {...props}
  />
));
SelectSeparator.displayName = SelectPrimitive.Separator.displayName;
