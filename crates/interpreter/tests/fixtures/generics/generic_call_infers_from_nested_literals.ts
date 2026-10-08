// A literal argument binds a type parameter wherever the parameter names it,
// as in tsc: in an object element of a tuple, in an object literal for an
// interface with methods, in an object a function literal returns, in a
// `Record` value, and in a field spread from another object, which types the
// callback fields after it.
interface Box<T> {
  v: T;
}

interface Getter<T> {
  v: T;
  get(): T;
}

function first<T>(pair: [{ v: T }, number]): T {
  return pair[0].v;
}

function get<T>(getter: Getter<T>): T {
  return getter.get();
}

function fromFactory<T>(make: () => Box<T>): T {
  return make().v;
}

function fromRecord<T>(record: Record<string, Box<T>>): T {
  return record["k"]!.v;
}

function run<T>(steps: { init: T; step: (x: T) => T }): T {
  return steps.step(steps.init);
}

function main(): void {
  assert(first([{ v: "x" }, 1]).length === 1, "a tuple element");
  assert(get({ v: 1, get() { return 2; } }).toFixed(1) === "2.0", "an interface with methods");
  assert(fromFactory(() => ({ v: "made" })).length === 4, "a function literal's returned object");
  assert(fromRecord({ k: { v: "rec" } }).length === 3, "a Record value");
  const base = { init: 5 };
  assert(run({ ...base, step: (x) => x - 1 }) === 4, "a spread field");
}
