import { cn } from "@/lib/utils";

type Props = {
  markdown: string;
  emptyLabel: string;
  className?: string;
};

type Block =
  | { type: "heading"; text: string }
  | { type: "list"; items: string[] }
  | { type: "paragraph"; text: string };

/** Parse the constrained CHANGELOG / GitHub-release body (### + bullets). */
export function parseReleaseNotes(markdown: string): Block[] {
  const lines = markdown.replace(/\r\n/g, "\n").trim().split("\n");
  const blocks: Block[] = [];
  let listItems: string[] = [];

  const flushList = () => {
    if (listItems.length === 0) return;
    blocks.push({ type: "list", items: listItems });
    listItems = [];
  };

  for (const raw of lines) {
    const line = raw.trimEnd();
    const trimmed = line.trim();
    if (!trimmed) {
      flushList();
      continue;
    }

    const heading = /^(#{1,3})\s+(.+)$/.exec(trimmed);
    if (heading) {
      flushList();
      blocks.push({ type: "heading", text: heading[2].trim() });
      continue;
    }

    const bullet = /^[-*+]\s+(.+)$/.exec(trimmed);
    if (bullet) {
      listItems.push(bullet[1].trim());
      continue;
    }

    flushList();
    blocks.push({ type: "paragraph", text: trimmed });
  }

  flushList();
  return blocks;
}

function InlineText({ text }: { text: string }) {
  // Support **bold** and `code` — enough for release notes.
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
          return (
            <strong key={i} className="font-medium text-foreground/90">
              {part.slice(2, -2)}
            </strong>
          );
        }
        if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
          return (
            <code
              key={i}
              className="rounded bg-muted/60 px-1 py-0.5 font-mono text-[0.85em]"
            >
              {part.slice(1, -1)}
            </code>
          );
        }
        return <span key={i}>{part}</span>;
      })}
    </>
  );
}

export function ReleaseNotes({ markdown, emptyLabel, className }: Props) {
  const trimmed = markdown.trim();
  if (!trimmed) {
    return (
      <p className={cn("text-xs text-muted", className)}>{emptyLabel}</p>
    );
  }

  const blocks = parseReleaseNotes(trimmed);

  return (
    <div
      className={cn(
        "max-h-44 space-y-2.5 overflow-y-auto pr-2.5 text-xs leading-relaxed text-muted [scrollbar-gutter:stable]",
        className,
      )}
    >
      {blocks.map((block, i) => {
        if (block.type === "heading") {
          return (
            <h4
              key={i}
              className="pt-0.5 text-[0.7rem] font-semibold uppercase tracking-wide text-foreground/80 first:pt-0"
            >
              <InlineText text={block.text} />
            </h4>
          );
        }
        if (block.type === "list") {
          return (
            <ul key={i} className="list-disc space-y-1 pl-4 marker:text-muted">
              {block.items.map((item, j) => (
                <li key={j} className="pl-0.5 [overflow-wrap:anywhere]">
                  <InlineText text={item} />
                </li>
              ))}
            </ul>
          );
        }
        return (
          <p key={i} className="[overflow-wrap:anywhere]">
            <InlineText text={block.text} />
          </p>
        );
      })}
    </div>
  );
}
