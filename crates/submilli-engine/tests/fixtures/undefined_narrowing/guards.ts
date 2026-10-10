function describe(value: string | null | undefined): string {
  if (value === undefined) { const missing: undefined = value; return "missing"; }
  if (value === null) { const empty: null = value; return "null"; }
  return value.toUpperCase();
}
function byType(value: unknown): string {
  if (typeof value === "undefined") { const missing: undefined = value; return "missing"; }
  if (typeof value === "string") { return value.toUpperCase(); }
  return "other";
}
function truthy(value: string | undefined | null): number {
  if (value) { return value.length; }
  return 0;
}
function choose(value: string | null | undefined): string {
  switch (value) {
    case undefined: return "missing";
    case null: return "null";
    default: return value.toUpperCase();
  }
}
function strictAliases(value: null | undefined): string {
  if (value === undefined) { const missing: undefined = value; return "missing"; }
  const empty: null = value;
  return "null";
}
function main(): void {
  assert(describe(undefined) === "missing");
  assert(describe(null) === "null");
  assert(describe("yes") === "YES");
  assert(byType(undefined) === "missing");
  assert(byType(null) === "other");
  assert(byType("yes") === "YES");
  assert(truthy(undefined) === 0);
  assert(truthy(null) === 0);
  assert(truthy("yes") === 3);
  assert(choose(undefined) === "missing");
  assert(choose(null) === "null");
  assert(choose("yes") === "YES");
  assert(strictAliases(null) === "null");
  assert(strictAliases(undefined) === "missing");
}
