function main(): void {
  const s: string | null = "";
  assert(!s, "empty string is falsy even behind string | null");
  const t: string | null = "x";
  assert(!!t, "non-empty string | null is truthy");
  const u: string | null = null;
  assert(!u, "null is falsy");
}
