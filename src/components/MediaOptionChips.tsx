import { Image, Video } from "lucide-react";
import { cn } from "@/lib/utils";

export type MediaOptionFlags = {
  handcam_foto?: boolean;
  handcam_video?: boolean;
  outside_foto?: boolean;
  outside_video?: boolean;
};

type MediaChipDef = {
  key: keyof Required<MediaOptionFlags>;
  source: "handcam" | "outside";
  kind: "foto" | "video";
  label: string;
  short: string;
};

const CHIPS: MediaChipDef[] = [
  {
    key: "handcam_foto",
    source: "handcam",
    kind: "foto",
    label: "Handcam Foto",
    short: "HC Foto",
  },
  {
    key: "handcam_video",
    source: "handcam",
    kind: "video",
    label: "Handcam Video",
    short: "HC Video",
  },
  {
    key: "outside_foto",
    source: "outside",
    kind: "foto",
    label: "Outside Foto",
    short: "OS Foto",
  },
  {
    key: "outside_video",
    source: "outside",
    kind: "video",
    label: "Outside Video",
    short: "OS Video",
  },
];

function sourceTone(source: MediaChipDef["source"]): string {
  // Handcam = teal, Outside = indigo — distinct from ID (sky) / offen (warning) / erledigt (success).
  if (source === "handcam") {
    return "border-teal-500/40 bg-teal-500/10 text-teal-800 dark:text-teal-200";
  }
  return "border-indigo-500/40 bg-indigo-500/10 text-indigo-800 dark:text-indigo-200";
}

export function activeMediaOptionKeys(
  flags: MediaOptionFlags,
): Array<MediaChipDef["key"]> {
  return CHIPS.filter((chip) => Boolean(flags[chip.key])).map((chip) => chip.key);
}

type Props = {
  flags: MediaOptionFlags;
  className?: string;
  /** Shorter labels for narrow rows (default: full labels). */
  compact?: boolean;
};

/** Booked Handcam/Outside Foto/Video chips for queue & review surfaces. */
export function MediaOptionChips({ flags, className, compact = false }: Props) {
  const active = CHIPS.filter((chip) => Boolean(flags[chip.key]));
  if (active.length === 0) return null;

  return (
    <ul
      className={cn("flex flex-wrap items-center gap-1", className)}
      aria-label="Gebuchte Medienoptionen"
    >
      {active.map((chip) => {
        const text = compact ? chip.short : chip.label;
        const Icon = chip.kind === "video" ? Video : Image;
        return (
          <li key={chip.key}>
            <span
              className={cn(
                "inline-flex max-w-full items-center gap-1 rounded border px-1.5 py-0.5 text-[10px] font-medium leading-none tracking-wide",
                sourceTone(chip.source),
              )}
              title={chip.label}
            >
              <Icon className="size-3 shrink-0 opacity-90" aria-hidden />
              <span className="truncate">{text}</span>
            </span>
          </li>
        );
      })}
    </ul>
  );
}
