import { open as openDirectoryDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Combobox, type ComboboxOption } from "@/components/ui/combobox";
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
  suggestions?: readonly ComboboxOption[];
  "aria-label"?: string;
};

async function pickShareDirectory(current: string): Promise<string | null> {
  try {
    const selected = await openDirectoryDialog({
      directory: true,
      multiple: false,
      defaultPath: current.trim() || undefined,
      title: "Ordner auswählen",
    });
    if (typeof selected === "string") return selected;
    return null;
  } catch {
    return null;
  }
}

/** Prefixed SMB/UNC/local path input (scheme picker + path body + folder picker). */
export function SmbUrlField({
  id,
  value,
  onChange,
  disabled = false,
  suggestions = [],
  "aria-label": ariaLabel = "Protokoll",
}: Props) {
  const { scheme, rest } = parseSmbUrlParts(value);
  const useSuggestions = suggestions.length > 0;

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
      {useSuggestions ? (
        <Combobox
          id={id}
          hideLabel
          embedded
          disabled={disabled}
          value={value}
          inputValue={rest}
          onInputValueChange={(nextRest) =>
            onChange(composeSmbUrl(scheme, nextRest))
          }
          onChange={onChange}
          onSelectOption={onChange}
          options={suggestions}
          placeholder={smbUrlRestPlaceholder(scheme)}
          listZIndex={200}
          inputClassName="h-9 rounded-none border-0 shadow-none focus-visible:ring-0"
          aria-label="Share-Pfad"
        />
      ) : (
        <Input
          id={id}
          disabled={disabled}
          className="h-9 min-w-0 flex-1 rounded-none border-0 shadow-none focus-visible:ring-0"
          value={rest}
          onChange={(e) => onChange(composeSmbUrl(scheme, e.target.value))}
          placeholder={smbUrlRestPlaceholder(scheme)}
          autoComplete="off"
          spellCheck={false}
        />
      )}
      <Button
        type="button"
        variant="ghost"
        size="icon"
        disabled={disabled}
        className="h-9 shrink-0 rounded-none rounded-r-md border-0 border-l border-border"
        title="Ordner wählen"
        onClick={() => {
          void pickShareDirectory(scheme === "local" ? rest : value).then((dir) => {
            if (dir) onChange(dir);
          });
        }}
      >
        <FolderOpen className="h-4 w-4" aria-hidden />
      </Button>
    </div>
  );
}
