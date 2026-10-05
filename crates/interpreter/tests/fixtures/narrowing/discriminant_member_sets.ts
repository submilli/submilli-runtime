// A discriminant test narrows a union to the members whose discriminant can
// hold the tested value, as in TypeScript: a member may type it with a
// literal union or a wider type, reach it through `?.`, or test it for
// truthiness. Through `?.`, a `null` object compares as `null`, since the
// chain short-circuits to `null` here, so `x?.kind === null` keeps it.
type Named = { type: "named"; name: string };
type Anonymous = { type: "anonymous" };
type Tagged = { kind: false; a: number } | { kind: true; b: string } | { kind: string; c: boolean };
type Grouped = { kind: "one"; one: number } | { kind: "two" | "three"; many: string };
type Unset = { kind: null; u: number };
type Assigned = { kind: "set"; s: string };
type Outcome = { error: null; value: number } | { error: Error };

function nameOf(arg: Named | Anonymous | null): string {
  if (arg?.type === "anonymous") {
    return "anonymous";
  }
  if (arg?.type !== "named") {
    return "none";
  }
  return arg.name;
}

function tagged(x: Tagged): string {
  if (x.kind === false) {
    return "a" + x.a.toString();
  }
  if (x.kind === true) {
    return x.b;
  }
  return "other";
}

function grouped(x: Grouped): string {
  if (x.kind === "three") {
    return x.many;
  }
  if (x.kind === "one") {
    return "n" + x.one.toString();
  }
  return "two:" + x.many;
}

function switchTagged(x: Tagged): string {
  switch (x.kind) {
    case false:
      return "a" + x.a.toString();
    case true:
      return x.b;
    default:
      return x.c ? "c" : "not c";
  }
}

function switchGrouped(x: Grouped): string {
  switch (x.kind) {
    case "one":
      return "n" + x.one.toString();
    case "two":
      return "two:" + x.many;
    default:
      return x.many;
  }
}

function outcome(x: Outcome): number {
  if (!x.error) {
    return x.value;
  }
  return -1;
}

function lengthIfThree(s: string | null): number {
  if (s?.length === 3) {
    return s.length;
  }
  if (s?.length) {
    return 0 - s.length;
  }
  return 0;
}

function unsetField(x: Assigned | Unset | null): number {
  if (x?.kind === null && x !== null) {
    return x.u;
  }
  return 0;
}

function main(): void {
  assert(nameOf({ type: "named", name: "n" }) === "n", "a `?.` discriminant narrows");
  assert(nameOf({ type: "anonymous" }) === "anonymous" && nameOf(null) === "none", "both ways");
  assert(tagged({ kind: false, a: 1 }) === "a1" && tagged({ kind: true, b: "b" }) === "b", "a wider member");
  assert(tagged({ kind: "s", c: true }) === "other", "keeps the wider member last");
  assert(grouped({ kind: "three", many: "m" }) === "m" && grouped({ kind: "one", one: 1 }) === "n1", "a literal union member");
  assert(grouped({ kind: "two", many: "t" }) === "two:t", "stays for a value it can still hold");
  assert(switchTagged({ kind: "s", c: true }) === "c" && switchTagged({ kind: true, b: "b" }) === "b", "a `switch` default keeps the wider member");
  assert(switchGrouped({ kind: "three", many: "m" }) === "m" && switchGrouped({ kind: "two", many: "t" }) === "two:t", "and the literal union member a case left open");
  assert(outcome({ error: null, value: 4 }) === 4 && outcome({ error: new Error("e") }) === -1, "a truthiness test");
  assert(unsetField({ kind: null, u: 1 }) === 1 && unsetField(null) === 0, "`?.` equal to `null`");
  assert(unsetField({ kind: "set", s: "s" }) === 0, "and not equal");
  assert(lengthIfThree("abc") === 3 && lengthIfThree("ab") === -2 && lengthIfThree(null) === 0, "a primitive's `?.`");
}
