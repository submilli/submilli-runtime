// Operators narrow as in TypeScript. `||` and `&&` split a `boolean` into
// `true` and `false`, `===` against a union narrows to the values both sides
// allow, `??` runs its right side where the left is `null`, an assignment
// tested inside `&&` narrows its target where the test holds, and a `switch`
// default drops the literals its cases matched.
type Message = { kind: "A"; x: string } | { kind: "B" | "C"; y: number } | { kind: "D" };

function orTrue(flag: boolean, n: number): true | number {
  return flag || n;
}

function andFalse(flag: boolean, s: string): false | string {
  return flag && s;
}

function equalToUnion(x: number, y: 1 | 2): 0 | 1 | 2 {
  if (x === 0 || x === y) {
    return x;
  }
  return 0;
}

function equalToMixedUnion(x: number | "foo" | "bar", y: 1 | 2 | string): "foo" | "bar" | 1 | 2 | "none" {
  if (x === y) {
    return x;
  }
  return "none";
}

function kindEqualToUnion(m: Message, k: "A" | "D"): string {
  if (m.kind === k) {
    const matched: "A" | "D" = m.kind;
    return matched;
  }
  return "other";
}

function onlyNull(value: null): string {
  return "null";
}

function coalesce(x: string | null): string {
  return x ?? onlyNull(x);
}

function assignedInAnd(x: number | string | boolean, c: boolean): number | false {
  return c && (x = 10) && x;
}

function switchDefault(x: number | "foo" | "bar"): number | "bar" {
  switch (x) {
    case "foo":
      return 0;
    default:
      return x;
  }
}

function main(): void {
  console.log(orTrue(true, 1), orTrue(false, 2));
  console.log(andFalse(true, "s"), andFalse(false, "s"));
  console.log(equalToUnion(0, 1), equalToUnion(1, 1), equalToUnion(5, 2));
  console.log(equalToMixedUnion("foo", "foo"), equalToMixedUnion(2, 2), equalToMixedUnion(5, 1));
  console.log(kindEqualToUnion({ kind: "A", x: "s" }, "A"), kindEqualToUnion({ kind: "B", y: 1 }, "D"));
  console.log(coalesce("a"), coalesce(null));
  console.log(assignedInAnd("s", true), assignedInAnd(1, false));
  console.log(switchDefault("foo"), switchDefault("bar"), switchDefault(3));
}
