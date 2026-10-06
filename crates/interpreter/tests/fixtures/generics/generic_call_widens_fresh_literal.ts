// A fresh literal type passed for a type parameter that isn't the whole
// result widens, as in tsc, so the result can hold other values: `box(label)`
// with `const label = "items"` is a `{ v: string }`. The same holds for a
// tuple element or an object-literal property typed by a type parameter, and
// for a type parameter several arguments share, unless an earlier argument
// already bound it to a type the literal fits: `two(mode, "off")` with
// `mode: Mode` is a `Mode`.
type Mode = "on" | "off";

class Box<T> {
  constructor(public value: T) {}
}

function box<T>(v: T): { v: T } {
  return { v: v };
}

function two<T>(x: T, y: T): T {
  return x;
}

function first<K>(pairs: [K, number][]): { key: K } {
  return { key: pairs[0][0] };
}

function wrapped<K>(o: { k: K }): K {
  return o.k;
}

function main(): void {
  const label = "items";
  const b = box(label);
  b.v = "other";
  assert(b.v === "other", "a field typed by the type parameter");

  const boxed = new Box(label);
  boxed.value = "other";
  assert(boxed.value === "other", "a class type argument");

  assert(two(label, "x") === "items", "two candidates");

  const m = new Map([[label, 1]]);
  m.set("other", 2);
  assert(m.size === 2, "a Map built from tuples");

  const f = first([[label, 1]]);
  f.key = "other";
  assert(f.key === "other", "a tuple element");

  let k = wrapped({ k: label });
  k = "other";
  assert(k === "other", "an object-literal property");

  const n = 1;
  const numbers = box(n);
  numbers.v = 5;
  assert(numbers.v === 5, "a number literal");

  const modes: Mode[] = ["on"];
  const current: Mode = modes[0];
  const mode = two(current, "off");
  assert(mode === "on", "a literal that fits the earlier binding");
}
