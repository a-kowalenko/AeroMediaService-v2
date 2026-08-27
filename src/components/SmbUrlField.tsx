import { Input } from "@/components/ui/input";
import { ServerUrlSchemePicker } from "@/components/ServerUrlSchemePicker";
import {
  composeSmbUrl,
  parseSmbUrlParts,
  smbUrlRestPlaceholder,
  smbUrlSchemeLabel,
  type SmbUrlScheme,
} from "@/lib/smbUrl";
import { cn } from "@/lib/utils";

type Props = {
  id?: string;
  value: string;
  onChange: (fullUrl: string) => void;
  disabled?: boolean;
  "aria-label"?: string;
};

/** Prefixed SMB/UNC URL input (scheme picker + path body). */
export function SmbUrlField({
  id,
  value,
  onChange,
  disabled = false,
  "aria-label": ariaLabel = "Protokoll",
}: Props) {
  const { scheme, rest } = parseSmbUrlParts(value);

  return (
    <div
      className={cn(
        "flex rounded-md border border-border bg-card shadow-sm",
        "focus-within:ring-2 focus-within:ring-ring",
        disabled && "opacity-50",
      )}
    >
      <ServerUrlSchemePicker
        value={scheme}
        disabled={disabled}
        aria-label={ariaLabel}
        labelFor={smbUrlSchemeLabel}
        onChange={(nextScheme: SmbUrlScheme) =>
          onChange(composeSmbUrl(nextScheme, rest))
        }
      />
      <Input
        id={id}
        disabled={disabled}
        className="h-9 min-w-0 flex-1 rounded-none rounded-r-md border-0 shadow-none focus-visible:ring-0"
        value={rest}
        onChange={(e) => onChange(composeSmbUrl(scheme, e.target.value))}
        placeholder={smbUrlRestPlaceholder(scheme)}
        autoComplete="off"
        spellCheck={false}
      />
    </div>
  );
}
