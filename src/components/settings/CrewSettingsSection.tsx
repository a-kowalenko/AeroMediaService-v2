import {useMemo, useState, type Dispatch, type SetStateAction} from "react";
import {Check, Pencil, Plus, Trash2, X} from "lucide-react";
import {SettingsSection} from "@/components/settings/SettingsSection";
import {Button} from "@/components/ui/button";
import {Checkbox} from "@/components/ui/checkbox";
import {Input} from "@/components/ui/input";
import {Label} from "@/components/ui/label";
import {
  cloneCrewMember,
  crewNamesEqual,
  emptyCrewDraft,
  type CrewMember,
} from "@/lib/crew";
import {cn} from "@/lib/utils";
import {useUiStore} from "@/store/uiStore";

type Props = {
  value: CrewMember[];
  onChange: (next: CrewMember[]) => void;
};

type Mode = {kind: "idle"} | {kind: "add"} | {kind: "edit"; index: number};

/** Split comma/semicolon lists into trimmed alias tokens. */
export function parseAliasInput(raw: string): string[] {
  return raw
    .split(/[,;]/)
    .map((a) => a.trim())
    .filter(Boolean);
}

function mergeAliases(existing: string[], incoming: string[]): string[] {
  const next = [...existing];
  for (const a of incoming) {
    if (!next.some((x) => crewNamesEqual(x, a))) {
      next.push(a);
    }
  }
  return next;
}

export function CrewSettingsSection({value, onChange}: Props) {
  const confirm = useUiStore((s) => s.confirm);
  const showWarning = useUiStore((s) => s.showWarning);

  const [mode, setMode] = useState<Mode>({kind: "idle"});
  const [draft, setDraft] = useState<CrewMember>(emptyCrewDraft);
  const [aliasInput, setAliasInput] = useState("");

  const sorted = useMemo(
    () =>
      value
        .map((member, index) => ({member, index}))
        .sort((a, b) => a.member.name.localeCompare(b.member.name, "de")),
    [value],
  );

  function resetEditor() {
    setMode({kind: "idle"});
    setDraft(emptyCrewDraft());
    setAliasInput("");
  }

  function startAdd() {
    setDraft(emptyCrewDraft());
    setAliasInput("");
    setMode({kind: "add"});
  }

  function startEdit(index: number) {
    const member = value[index];
    if (!member) return;
    setDraft(cloneCrewMember(member));
    setAliasInput("");
    setMode({kind: "edit", index});
  }

  function commitAliasesFromInput(current: CrewMember): CrewMember {
    const tokens = parseAliasInput(aliasInput);
    if (tokens.length === 0) return current;
    return {...current, aliases: mergeAliases(current.aliases, tokens)};
  }

  function addAliasesFromInput() {
    const tokens = parseAliasInput(aliasInput);
    if (tokens.length === 0) return;
    setDraft((prev) => ({
      ...prev,
      aliases: mergeAliases(prev.aliases, tokens),
    }));
    setAliasInput("");
  }

  function removeAlias(alias: string) {
    setDraft((prev) => ({
      ...prev,
      aliases: prev.aliases.filter((x) => !crewNamesEqual(x, alias)),
    }));
  }

  function saveMember() {
    const withAliases = commitAliasesFromInput(draft);
    const name = withAliases.name.trim();
    if (!name) {
      showWarning("Bitte einen Namen eingeben.");
      return;
    }
    if (!withAliases.tandemmaster && !withAliases.videospringer) {
      showWarning("Mindestens eine Rolle (TM oder VS) setzen.");
      return;
    }
    const editIndex = mode.kind === "edit" ? mode.index : null;
    const duplicate = value.some(
      (m, i) => i !== editIndex && crewNamesEqual(m.name, name),
    );
    if (duplicate) {
      showWarning("Dieser Name ist bereits in der Crew-Liste.");
      return;
    }
    const nextMember: CrewMember = {
      name,
      tandemmaster: withAliases.tandemmaster,
      videospringer: withAliases.videospringer,
      aliases: withAliases.aliases
        .map((a) => a.trim())
        .filter(Boolean)
        .filter(
          (a, i, arr) =>
            arr.findIndex((x) => crewNamesEqual(x, a)) === i,
        ),
    };
    if (editIndex == null) {
      onChange([...value, nextMember]);
    } else {
      onChange(value.map((m, i) => (i === editIndex ? nextMember : m)));
    }
    resetEditor();
  }

  async function deleteMember(index: number) {
    const member = value[index];
    if (!member) return;
    const ok = await confirm(`„${member.name}“ aus der Crew entfernen?`, {
      title: "Crew",
      primaryLabel: "Entfernen",
      secondaryLabel: "Abbrechen",
      destructive: true,
    });
    if (!ok) return;
    onChange(value.filter((_, i) => i !== index));
    if (mode.kind === "edit" && mode.index === index) resetEditor();
    else if (mode.kind === "edit" && mode.index > index) {
      setMode({kind: "edit", index: mode.index - 1});
    }
  }

  function patchRole(
    index: number,
    role: "tandemmaster" | "videospringer",
    checked: boolean,
  ) {
    onChange(
      value.map((m, i) => {
        if (i !== index) return m;
        const next = {...m, [role]: checked};
        if (!next.tandemmaster && !next.videospringer) {
          // Keep at least one role.
          return m;
        }
        return next;
      }),
    );
  }

  const editing = mode.kind !== "idle";

  return (
    <SettingsSection
      title="Crew-Liste"
      description="Tandemmaster und Videospringer für Ordnername-Vorhersage. Aliases (z. B. Corni → Cornelius) werden im Predictor mitgematcht. Kurze, eindeutige Aliase bevorzugen — Gast-Kollisionen fängt der Predictor über die TA-Zone ab, nicht durch Alias-Löschen."
    >
      <div className="mb-3 flex items-center justify-between gap-2">
        <p className="text-xs text-muted">
          {value.length} Personen — Rollen und Aliases hier pflegen.
        </p>
        {mode.kind !== "add" ? (
          <Button type="button" size="sm" onClick={startAdd}>
            <Plus className="size-4" />
            Hinzufügen
          </Button>
        ) : null}
      </div>

      <ul className="divide-y divide-border rounded-lg border border-border">
        {mode.kind === "add" ? (
          <li className="bg-muted/30 px-3 py-3">
            <MemberEditor
              draft={draft}
              setDraft={setDraft}
              aliasInput={aliasInput}
              setAliasInput={setAliasInput}
              onAddAliases={addAliasesFromInput}
              onRemoveAlias={removeAlias}
              onSave={saveMember}
              onCancel={resetEditor}
            />
          </li>
        ) : null}

        {sorted.map(({member, index}) => {
          const isEditing = mode.kind === "edit" && mode.index === index;
          return (
            <li
              key={`${member.name}-${index}`}
              className={cn(
                "px-3 py-2",
                isEditing && "bg-muted/30 py-3",
              )}
            >
              {isEditing ? (
                <MemberEditor
                  draft={draft}
                  setDraft={setDraft}
                  aliasInput={aliasInput}
                  setAliasInput={setAliasInput}
                  onAddAliases={addAliasesFromInput}
                  onRemoveAlias={removeAlias}
                  onSave={saveMember}
                  onCancel={resetEditor}
                />
              ) : (
                <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                  <div className="min-w-0 space-y-1">
                    <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                      <div className="truncate text-sm font-medium">
                        {member.name}
                      </div>
                      {member.aliases.map((alias) => (
                        <span
                          key={alias}
                          className="inline-flex shrink-0 items-center rounded-md border border-border bg-background/70 px-2 py-0.5 text-xs text-muted"
                        >
                          {alias}
                        </span>
                      ))}
                    </div>
                    <div className="flex flex-wrap gap-3 text-xs">
                      <label className="flex items-center gap-1.5">
                        <Checkbox
                          checked={member.tandemmaster}
                          disabled={editing}
                          onCheckedChange={(v) =>
                            patchRole(index, "tandemmaster", Boolean(v))
                          }
                        />
                        TM
                      </label>
                      <label className="flex items-center gap-1.5">
                        <Checkbox
                          checked={member.videospringer}
                          disabled={editing}
                          onCheckedChange={(v) =>
                            patchRole(index, "videospringer", Boolean(v))
                          }
                        />
                        VS
                      </label>
                    </div>
                  </div>
                  <div className="flex shrink-0 gap-0.5">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      disabled={editing}
                      onClick={() => startEdit(index)}
                      title="Bearbeiten"
                      aria-label="Bearbeiten"
                    >
                      <Pencil className="size-3.5" />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-destructive hover:text-destructive"
                      disabled={editing}
                      onClick={() => void deleteMember(index)}
                      title="Entfernen"
                      aria-label="Entfernen"
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                </div>
              )}
            </li>
          );
        })}

        {sorted.length === 0 && mode.kind !== "add" ? (
          <li className="px-3 py-4 text-sm text-muted">
            Keine Crew-Einträge.{" "}
            <button
              type="button"
              className="text-primary underline-offset-2 hover:underline"
              onClick={startAdd}
            >
              Erste Person hinzufügen
            </button>
          </li>
        ) : null}
      </ul>
    </SettingsSection>
  );
}

type MemberEditorProps = {
  draft: CrewMember;
  setDraft: Dispatch<SetStateAction<CrewMember>>;
  aliasInput: string;
  setAliasInput: (v: string) => void;
  onAddAliases: () => void;
  onRemoveAlias: (alias: string) => void;
  onSave: () => void;
  onCancel: () => void;
};

function MemberEditor({
  draft,
  setDraft,
  aliasInput,
  setAliasInput,
  onAddAliases,
  onRemoveAlias,
  onSave,
  onCancel,
}: MemberEditorProps) {
  return (
    <div
      className="space-y-3"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onCancel();
        }
      }}
    >
      <div className="grid gap-3 sm:grid-cols-[1fr_auto]">
        <div className="space-y-1.5">
          <Label>Name</Label>
          <Input
            autoFocus
            value={draft.name}
            onChange={(e) =>
              setDraft((prev) => ({...prev, name: e.target.value}))
            }
            placeholder="z. B. Cornelius"
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onSave();
              }
            }}
          />
        </div>
        <div className="flex items-end gap-1">
          <Button
            type="button"
            size="icon"
            className="h-9 w-9"
            onClick={onSave}
            title="Speichern"
            aria-label="Speichern"
          >
            <Check className="size-4" />
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="icon"
            className="h-9 w-9"
            onClick={onCancel}
            title="Abbrechen"
            aria-label="Abbrechen"
          >
            <X className="size-4" />
          </Button>
        </div>
      </div>

      <div className="flex flex-wrap gap-4">
        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={draft.tandemmaster}
            onCheckedChange={(v) =>
              setDraft((prev) => ({...prev, tandemmaster: Boolean(v)}))
            }
          />
          Tandemmaster
        </label>
        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={draft.videospringer}
            onCheckedChange={(v) =>
              setDraft((prev) => ({...prev, videospringer: Boolean(v)}))
            }
          />
          Videospringer
        </label>
      </div>

      <div className="space-y-2">
        <Label>Aliases</Label>
        <div className="flex gap-2">
          <Input
            value={aliasInput}
            onChange={(e) => setAliasInput(e.target.value)}
            placeholder="z. B. Corni, Corny"
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onAddAliases();
              }
            }}
          />
          <Button type="button" variant="secondary" onClick={onAddAliases}>
            Alias +
          </Button>
        </div>
        {draft.aliases.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {draft.aliases.map((alias) => (
              <span
                key={alias}
                className="inline-flex items-center gap-1 rounded-md border border-border bg-background/70 px-2 py-0.5 text-xs"
              >
                {alias}
                <button
                  type="button"
                  className="text-muted hover:text-foreground"
                  onClick={() => onRemoveAlias(alias)}
                  aria-label={`Alias ${alias} entfernen`}
                >
                  <X className="size-3" />
                </button>
              </span>
            ))}
          </div>
        ) : (
          <p className="text-xs text-muted">
            Mehrere Aliases kommasepariert möglich.
          </p>
        )}
      </div>
    </div>
  );
}
