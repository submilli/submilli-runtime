// A `switch` whose cases name every value of the discriminant's declared type
// leaves `never` in its `default`, on a field, `this`, or a module variable a
// function assigns as well as on a local, so `assertNever` checks there.
type Shape = { kind: "c"; r: number } | { kind: "s"; a: number };
function assertNever(x: never): never {
  throw new Error("unexpected value");
}
function onField(o: { s: Shape }): number {
  switch (o.s.kind) {
    case "c": return 1;
    case "s": return 2;
    default: return assertNever(o.s);
  }
}
function onKind(o: { k: "a" | "b" }): number {
  switch (o.k) {
    case "a": return 1;
    case "b": return 2;
    default: return assertNever(o.k);
  }
}
enum E { A, B }
function onEnumField(o: { e: E }): number {
  switch (o.e) {
    case E.A: return 1;
    case E.B: return 2;
    default: return assertNever(o.e);
  }
}
class C {
  k: "a" | "b" = "a";
  m(): number {
    switch (this.k) {
      case "a": return 1;
      case "b": return 2;
      default: return assertNever(this.k);
    }
  }
}
let g: "a" | "b" = "a";
function onGlobal(): number {
  switch (g) {
    case "a": return 1;
    case "b": return 2;
    default: return assertNever(g);
  }
}
function setG(): void {
  g = "b";
}
function main(): void {
  assert(onField({ s: { kind: "c", r: 1 } }) === 1);
  assert(onKind({ k: "b" }) === 2);
  assert(onEnumField({ e: E.B }) === 2);
  assert(new C().m() === 1);
  setG();
  assert(onGlobal() === 2);
  console.log("ok");
}
