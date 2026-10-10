// A tuple position may be `undefined` in one member, like any literal: a
// `switch` on it narrows each case, including `case undefined`.
type T = [undefined, number] | ["s", string] | [null, boolean];
function describe(t: T): string {
  switch (t[0]) {
    case undefined:
      return `n${t[1]}`;
    case "s":
      return t[1];
    case null:
      return `b${t[1]}`;
  }
}
function main(): void {
  assert(describe([undefined, 1]) === "n1", "the undefined position narrows");
  assert(describe(["s", "x"]) === "x", "a string position narrows");
  assert(describe([null, true]) === "btrue", "a null position narrows");
}
