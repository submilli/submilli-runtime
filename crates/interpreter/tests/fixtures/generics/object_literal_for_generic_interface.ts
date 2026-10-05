// An object literal passed for a generic interface binds the interface's type
// parameters from its own fields, as a variable holding the same object does:
// `unbox({ v: "s" })` is a `string`. So do literals nested in its fields, in
// an array or tuple of the interface, for a nullable interface, in
// parentheses or a conditional, and a recursive interface's own literal.
interface Box<T> {
  v: T;
}

interface Labeled<T> {
  label?: T;
  count: number;
}

interface Nest<T> {
  inner: Box<T>;
}

interface Tree<T> {
  v: T;
  kids: Tree<T>[];
}

function unbox<T>(box: Box<T>): T {
  return box.v;
}

function label<T>(labeled: Labeled<T>): T | null {
  return labeled.label ?? null;
}

function deep<T>(nest: Nest<T>): T {
  return nest.inner.v;
}

function unboxAll<T>(boxes: Box<T>[]): T[] {
  return boxes.map((box) => box.v);
}

function maybe<T>(box: Box<T> | null): T | null {
  return box === null ? null : box.v;
}

function first<T>(pair: [Box<T>, number]): T {
  return pair[0].v;
}

function root<T>(tree: Tree<T>): T {
  return tree.v;
}

function main(): void {
  const word = unbox({ v: "s" });
  const flag = label({ label: true, count: 1 });
  const items = deep({ inner: { v: [1, 2] } });
  const letters = unboxAll([{ v: "a" }, { v: "b" }]);
  const top = root({ v: 1, kids: [{ v: 2, kids: [] }] });
  assert(word.length === 1 && flag === true, "fields");
  assert(items.length === 2 && letters.join(",") === "a,b", "nested literals");
  assert(top + 1 === 2, "a recursive interface");
  const found = maybe({ v: "m" });
  const wrapped = unbox(({ v: "p" }));
  const chosen = unbox(word.length > 0 ? { v: "a" } : { v: "b" });
  assert(found !== null && found.length === 1, "a nullable interface");
  assert(wrapped.length + chosen.length === 2, "parentheses and a conditional");
}
