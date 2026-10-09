// A `case null:` does not cover `undefined`: an `undefined` discriminant falls
// through the switch, as in TypeScript.
function pick(x: "a" | null | undefined): string {
  switch (x) { case "a": return "A"; case null: return "N"; }
  return "?";
}
function main(): void {
  assert(pick(null) === "N", "null has its case");
  assert(pick(undefined) === "?", "undefined falls through");
  console.log(pick("a"), pick(null), pick(undefined));
}
