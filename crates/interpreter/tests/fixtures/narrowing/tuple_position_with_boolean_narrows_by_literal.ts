// A tuple position one member holds as `boolean` still narrows the tuple by
// a literal compared with it, in a `case` as in an `if`.
type Pair = [null, string] | [boolean, number];
function byCase(v: Pair): string {
  switch (v[0]) {
    case true:
      return String(v[1] + 1);
    default:
      return "other";
  }
}
function byIf(v: Pair): string {
  if (v[0] === false) return String(v[1] + 2);
  if (v[0] === null) return v[1];
  return "true";
}
function main(): void {
  assert(byCase([true, 1]) === "2");
  assert(byCase([null, "x"]) === "other");
  assert(byIf([false, 1]) === "3");
  assert(byIf([null, "y"]) === "y");
  assert(byIf([true, 3]) === "true");
}
