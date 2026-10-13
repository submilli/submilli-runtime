function orDefault(s: string | null): string {
  return s || "default";
}

function main(): void {
  assert(orDefault(null) === "default", "null takes the fallback");
  assert(orDefault("") === "default", "empty string takes the fallback (unlike ??)");
  assert(orDefault("x") === "x", "truthy string is kept");
  const empty: string = "";
  const none: null = null as null;
  const first: string = "first";
  const a: string = empty || "x";
  assert(a === "x", "empty string literal falls through");
  const b: number = 0 || 5;
  assert(b === 5, "zero falls through");
  const c: string = none || "y";
  assert(c === "y", "null falls through");
  const d: string = empty || (empty || "z");
  assert(d === "z", "chained || walks all falsy operands");
  const kept: string = first || "second";
  assert(kept === "first", "truthy lhs short-circuits");
}
