// Several members may read `undefined` for the same discriminant: two optional
// ones, or an optional one next to `k: undefined`. A test for another literal
// still narrows to its member, and a test for `undefined` keeps every member
// that may omit the field, as in TypeScript.
type S = { k?: "a"; x: number } | { k?: "c"; z: boolean } | { k: "b"; y: string };
type T = { k?: "a"; x: number } | { k: undefined; z: boolean } | { k: "b"; y: string };
function s(v: S): string {
  if (v.k === "b") {
    return v.y;
  }
  if (v.k === "a") {
    return `a${v.x}`;
  }
  if (v.k === "c") {
    return `c${v.z}`;
  }
  return "absent";
}
function t(v: T): string {
  switch (v.k) {
    case "b":
      return v.y;
    case "a":
      return `a${v.x}`;
    default:
      return "absent or undefined";
  }
}
function main(): void {
  assert(s({ k: "b", y: "Y" }) === "Y", "an explicit literal narrows past two optional members");
  assert(s({ k: "a", x: 1 }) === "a1", "an optional member's literal narrows to it");
  assert(s({ x: 2 }) === "absent", "an omitted discriminant matches no literal");
  assert(t({ k: "b", y: "Z" }) === "Z", "switch narrows past an explicit undefined member");
  assert(t({ k: undefined, z: true }) === "absent or undefined", "undefined falls through");
}
