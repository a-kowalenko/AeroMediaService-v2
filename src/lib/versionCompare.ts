/** Loose numeric version compare (0.1.10 > 0.1.9). Returns <0, 0, >0. */
export function compareVersionParts(a: string, b: string): number {
  const parse = (v: string) =>
    v
      .split(/[^\d]+/)
      .filter(Boolean)
      .map((s) => Number.parseInt(s, 10))
      .filter((n) => Number.isFinite(n));
  const left = parse(a);
  const right = parse(b);
  const len = Math.max(left.length, right.length);
  for (let i = 0; i < len; i++) {
    const l = left[i] ?? 0;
    const r = right[i] ?? 0;
    if (l !== r) return l - r;
  }
  return 0;
}
