// Interface members with an optional literal field beside the tag narrow by
// whichever field a test names: the tag, or the optional field itself.
interface A { code?: 1; kind: "a"; n: number; }
interface B { code?: 2; kind: "b"; s: string; }
type S = A | B;
function byKind(v: S): string {
  if (v.kind === "a") {
    return `${v.n}`;
  }
  return v.s;
}
function byCode(v: S): string {
  switch (v.code) {
    case 1:
      return `${v.n}`;
    case 2:
      return v.s;
    case undefined:
      return "no code";
  }
}
function main(): void {
  assert(byKind({ kind: "a", n: 1 }) === "1", "an interface union narrows by its tag");
  assert(byCode({ code: 2, kind: "b", s: "B" }) === "B", "a test on the optional field narrows by it");
  assert(byCode({ kind: "a", n: 3 }) === "no code", "an omitted optional field is undefined");
}
