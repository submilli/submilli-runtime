function length(s: string | null): number {
  switch (s) {
    case null: return -1;
    default: return s.length;
  }
}
function literal(s: "one" | "two" | null): string {
  switch (s) {
    case "one": return "first";
    case null: return "nil";
    default: return s;
  }
}
function partial(s: string | null): string {
  switch (s) { case null: return "nil"; }
  return "other";
}
export function main(): void {
  assert(partial(null) === "nil", "partial switch null case");
  assert(partial("yes") === "other", "open switch may omit default");
  assert(length("yes") === 3, "default excludes null");
  assert(length("") === 0, "empty string is not null");
  assert(length(null) === -1, "null case");
  assert(literal("one") === "first", "literal case");
  assert(literal("two") === "two", "remaining literal");
  assert(literal(null) === "nil", "nullable literal union");
}
