function five(o: { mid: { leaf: string | null } | null }): string {
 if (o.mid !== null && o.mid.leaf !== null) { return o.mid.leaf; } return "none";
}
function main(): void {
  assert(five({ mid: { leaf: "L" } }) === "L");
  assert(five({ mid: null }) === "none");
  assert(five({ mid: { leaf: null } }) === "none");
  assert(nested({ mid: { leaf: "nested" } }) === "nested");
  assert(nested({ mid: null }) === "none");
  assert(nested({ mid: { leaf: null } }) === "none");
}
type Mid = { leaf: string | null };
function nested(o: { mid: Mid | null }): string {
  if (o.mid !== null) { if (o.mid.leaf !== null) { return o.mid.leaf; } }
  return "none";
}
