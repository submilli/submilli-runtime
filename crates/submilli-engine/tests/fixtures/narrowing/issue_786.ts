function nested(x: string | null): string {
  { { if (x === null) { return "L"; } } }
  return "b" + x;
}
function joined(x: string | null, flag: boolean): string {
  if (flag) { if (x === null) { return "L"; } }
  else { if (x === null) { return "R"; } }
  return x;
}
interface Box { value: string | null; }
function field(b: Box): number {
  { if (b.value === null) { return -1; } }
  return b.value.length;
}
function shadow(x: string | null): string {
  { const x: string | null = "inner"; if (x === null) { return "impossible"; } }
  if (x === null) { return "outer"; }
  return x;
}
export function main(): void {
  assert(nested("yes") === "byes", "nested block retains guard");
  assert(nested(null) === "L", "nested return");
  assert(joined("yes", true) === "yes", "then exit");
  assert(joined("yes", false) === "yes", "else exit");
  assert(joined(null, true) === "L", "then return");
  assert(joined(null, false) === "R", "else return");
  assert(field({value:"abc"}) === 3, "field fact crosses block");
  assert(field({value:null}) === -1, "field null return");
  assert(shadow(null) === "outer", "inner binding does not narrow outer");
}
