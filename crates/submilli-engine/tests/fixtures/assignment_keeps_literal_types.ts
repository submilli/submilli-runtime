// An assignment has its value's type, literal types included, as in
// TypeScript: `(x = 10)` is `10`. What the target then reads as follows
// TypeScript's narrowing by assignment: a fresh literal the declared type
// doesn't name widens (`x` reads as `number`), and a `boolean` narrows to the
// `true` or `false` written to it, from its initializer too.
function pick(): boolean {
  return "ab".length === 2;
}
function ten(t: 10): number {
  return t;
}
function onlyTrue(t: true): number {
  return t ? 1 : 0;
}
function onlyFalse(f: false): number {
  return f ? 1 : 2;
}
function word(s: "s"): number {
  return s.length;
}

function main(): void {
  let x: number | string = pick() ? "a" : "b";
  const assigned = (x = 10);
  assert(ten(assigned) === 10, "an assignment's value keeps its literal type");
  x = x + 1;
  assert(x === 11, "the target reads as `number`, not `10`");

  let ready = true;
  const both = ready && "s";
  assert(word(both) === 1, "an initializer narrows a `boolean`");

  let toggled: boolean = pick();
  toggled = true;
  assert(onlyTrue(toggled) === 1, "an assignment narrows a `boolean`");

  let cleared: boolean | null = null;
  cleared = false;
  assert(onlyFalse(cleared) === 2, "and a `boolean | null`");

  let chained: number | string = "c";
  let other: number | string = "d";
  chained = other = 5;
  chained = chained + 1;
  assert(chained === 6 && other === 5, "a chained assignment's literal widens in its target");
}
