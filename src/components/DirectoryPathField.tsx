import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

type Props = {
  label: string;
  value: string;
  placeholder?: string;
  disabled?: boolean;
  onChange: (v: string) => void;
  onPick: () => void;
  id?: string;
};

/** Path text input with inline folder picker (same chrome as SmbUrlField / Combobox row). */
export function DirectoryPathField({
  label,
  value,
  placeholder,
  disabled = false,
  onChange,
  onPick,
  id,
}: Props) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <div
        className={cn(
          "flex rounded-md border border-border bg-card shadow-sm",
          "focus-within:ring-2 focus-within:ring-ring",
          disabled && "opacity-50",
        )}
      >
        <Input
          id={id}
          disabled={disabled}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          className="h-9 min-w-0 flex-1 rounded-none border-0 shadow-none focus-visible:ring-0"
          autoComplete="off"
          spellCheck={false}
        />
        <Button
          type="button"
          variant="ghost"
          size="icon"
          disabled={disabled}
          className="h-9 shrink-0 rounded-none rounded-r-md border-0 border-l border-border"
          title="Ordner wählen"
          onClick={onPick}
        >
          <FolderOpen className="h-4 w-4" aria-hidden />
        </Button>
      </div>
    </div>
  );
}
