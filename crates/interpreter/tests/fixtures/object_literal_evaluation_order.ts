// An object literal evaluates its members in source order, as JavaScript does,
// including a value a later spread overwrites. A spread's optional field that is
// absent leaves the earlier value in place, so the field holds either type.
let log: string = "";

function mark(label: string): number {
  log = log + label;
  return 1;
}

function source(): { c: number } {
  log = log + "S";
  return { c: 3 };
}

function main(): void {
  const plain = { b: mark("b"), a: mark("a") };
  assert(log === "ba", "fields evaluate in source order");
  assert(plain.a + plain.b === 2, "both fields are set");

  log = "";
  const spread = { z: mark("z"), ...source(), y: mark("y") };
  assert(log === "zSy", "a spread evaluates where it is written");
  assert(spread.c === 3, "the spread's field is copied");

  log = "";
  const full: { a: number } = { a: 2 };
  const overwritten = { a: mark("x"), ...full };
  assert(log === "x", "an overwritten value still runs");
  assert(overwritten.a === 2, "the spread's value wins");

  const absent: { a?: string } = {};
  const kept = { a: 123, ...absent };
  const keptValue: number | string = kept.a;
  assert(keptValue === 123, "an absent optional field keeps the earlier value");

  const present: { a?: string } = { a: "x" };
  const replaced = { a: 123, ...present };
  assert(replaced.a === "x", "a present optional field replaces it");

  const alsoAbsent: { a?: number } = {};
  const neither = { ...alsoAbsent, ...absent };
  assert((neither.a ?? "none") === "none", "optional over optional stays absent");
}
