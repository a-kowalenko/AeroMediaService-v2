/** Crew roster helpers (Phase 19a) — mirrors ATS DEFAULT_CREW_LIST + aliases. */

export type CrewMember = {
  name: string;
  tandemmaster: boolean;
  videospringer: boolean;
  aliases: string[];
};

export const DEFAULT_CREW_LIST: CrewMember[] = [
  {name: "Alberto", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Ana", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Andy", tandemmaster: true, videospringer: true, aliases: ["Andreas"]},
  {name: "Chris", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Cornelius", tandemmaster: true, videospringer: false, aliases: ["Corni"]},
  {name: "Futti", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Harry", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Henrik", tandemmaster: true, videospringer: true, aliases: ["Henni"]},
  {name: "Jan", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Jojo", tandemmaster: false, videospringer: true, aliases: []},
  {name: "Kai", tandemmaster: false, videospringer: true, aliases: []},
  {name: "Käthe", tandemmaster: false, videospringer: true, aliases: []},
  {name: "Mathi", tandemmaster: true, videospringer: true, aliases: ["Mathias"]},
  {name: "Max", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Mayo", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Pascal", tandemmaster: true, videospringer: false, aliases: ["Passy"]},
  {name: "Ralph", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Rene", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Robert", tandemmaster: false, videospringer: true, aliases: []},
  {name: "Robin", tandemmaster: false, videospringer: true, aliases: []},
  {name: "Sabrina", tandemmaster: false, videospringer: true, aliases: []},
  {name: "Sahira", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Samuel", tandemmaster: true, videospringer: true, aliases: ["Samu"]},
  {name: "Stefan", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Steve", tandemmaster: true, videospringer: false, aliases: []},
  {name: "Tim", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Tom", tandemmaster: true, videospringer: true, aliases: []},
  {name: "Torsten", tandemmaster: true, videospringer: true, aliases: []},
].sort((a, b) => a.name.localeCompare(b.name, "de"));

function normalizeMember(raw: unknown): CrewMember | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const name = typeof o.name === "string" ? o.name.trim() : "";
  if (!name) return null;
  const aliases = Array.isArray(o.aliases)
    ? o.aliases
        .filter((a): a is string => typeof a === "string")
        .map((a) => a.trim())
        .filter(Boolean)
    : [];
  return {
    name,
    tandemmaster: Boolean(o.tandemmaster),
    videospringer: Boolean(o.videospringer),
    aliases,
  };
}

/** Parse `crew_list` setting JSON; empty/invalid → defaults. */
export function parseCrewList(raw: string | null | undefined): CrewMember[] {
  const trimmed = (raw ?? "").trim();
  if (!trimmed) return DEFAULT_CREW_LIST.map(cloneCrewMember);
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!Array.isArray(parsed) || parsed.length === 0) {
      return DEFAULT_CREW_LIST.map(cloneCrewMember);
    }
    const list = parsed
      .map(normalizeMember)
      .filter((m): m is CrewMember => m !== null);
    return list.length > 0 ? list : DEFAULT_CREW_LIST.map(cloneCrewMember);
  } catch {
    return DEFAULT_CREW_LIST.map(cloneCrewMember);
  }
}

export function serializeCrewList(list: CrewMember[]): string {
  return JSON.stringify(list);
}

export function cloneCrewMember(m: CrewMember): CrewMember {
  return {
    name: m.name,
    tandemmaster: m.tandemmaster,
    videospringer: m.videospringer,
    aliases: [...m.aliases],
  };
}

export function emptyCrewDraft(): CrewMember {
  return {name: "", tandemmaster: true, videospringer: false, aliases: []};
}

export function crewNamesEqual(
  a: string | null | undefined,
  b: string | null | undefined,
): boolean {
  const left = (a ?? "").trim().toLowerCase();
  const right = (b ?? "").trim().toLowerCase();
  return Boolean(left) && left === right;
}

export function crewNamesForRole(
  list: CrewMember[],
  role: "tandemmaster" | "videospringer",
): string[] {
  return list
    .filter((c) => (role === "tandemmaster" ? c.tandemmaster : c.videospringer))
    .map((c) => c.name)
    .sort((a, b) => a.localeCompare(b, "de"));
}

/** Ensure the current value appears in options even if not in the filtered role list. */
export function crewSelectOptions(
  list: CrewMember[],
  role: "tandemmaster" | "videospringer",
  current: string | null | undefined,
): string[] {
  const names = crewNamesForRole(list, role);
  const trimmed = (current ?? "").trim();
  if (trimmed && !names.some((n) => crewNamesEqual(n, trimmed))) {
    return [trimmed, ...names];
  }
  return names;
}
