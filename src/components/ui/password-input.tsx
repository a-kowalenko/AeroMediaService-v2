import * as React from "react";
import { Eye, EyeOff } from "lucide-react";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type Props = Omit<React.ComponentProps<"input">, "type"> & {
  /** Controlled visibility; omit for internal state. */
  visible?: boolean;
  onVisibleChange?: (visible: boolean) => void;
};

/**
 * Password field with a Lucide Eye toggle. Hides the Edge/Chromium native
 * `::-ms-reveal` so only one reveal control is shown.
 */
export const PasswordInput = React.forwardRef<HTMLInputElement, Props>(
  (
    { className, visible: visibleProp, onVisibleChange, ...props },
    ref,
  ) => {
    const [uncontrolledVisible, setUncontrolledVisible] = React.useState(false);
    const visible = visibleProp ?? uncontrolledVisible;

    function setVisible(next: boolean) {
      onVisibleChange?.(next);
      if (visibleProp === undefined) setUncontrolledVisible(next);
    }

    return (
      <div className="relative">
        <Input
          ref={ref}
          type={visible ? "text" : "password"}
          className={cn("pr-9 [&::-ms-reveal]:hidden [&::-ms-clear]:hidden", className)}
          {...props}
        />
        <button
          type="button"
          tabIndex={-1}
          onClick={() => setVisible(!visible)}
          title={visible ? "Passwort verbergen" : "Passwort anzeigen"}
          aria-label={visible ? "Passwort verbergen" : "Passwort anzeigen"}
          className={cn(
            "absolute top-1/2 right-1 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded text-muted transition-colors",
            "hover:bg-primary-soft hover:text-foreground",
          )}
        >
          {visible ? (
            <EyeOff className="h-3.5 w-3.5" aria-hidden />
          ) : (
            <Eye className="h-3.5 w-3.5" aria-hidden />
          )}
        </button>
      </div>
    );
  },
);
PasswordInput.displayName = "PasswordInput";
