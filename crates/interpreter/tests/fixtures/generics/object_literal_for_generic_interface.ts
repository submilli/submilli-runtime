// An object literal passed for a generic interface binds the interface's type
// parameters from its own fields, as a variable holding the same object does:
// `unbox({ v: "s" })` is a `string`. So do literals nested in its fields, in
// an array or tuple of the interface, for a nullable interface, in
// parentheses or a conditional, and a recursive interface's own literal,
// including interfaces that name each other in a union, and a union member
// whose fields hold another member, however deep, or many such members.
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

interface Red<T> {
  red: T;
  next: Red<T> | Green<T> | Blue<T> | null;
}

interface Green<T> {
  green: T;
  next: Red<T> | Green<T> | Blue<T> | null;
}

interface Blue<T> {
  blue: T;
  next: Red<T> | Green<T> | Blue<T> | null;
}

function lead<T>(chain: Red<T> | Green<T> | Blue<T>): T | null {
  return null;
}

interface Leaf<T> {
  leaf: T;
}

interface Pair<T> {
  left: Leaf<T>;
  right: Leaf<T>;
}

function leftmost<T>(node: Leaf<T> | Pair<T>): T {
  return "leaf" in node ? node.leaf : node.left.leaf;
}

interface Outer<T> {
  a: Middle<T>;
}

interface Middle<T> {
  b: Inner<T>;
}

interface Inner<T> {
  c: Box<T>;
}

function innermost<T>(outer: Outer<T>): T {
  return outer.a.b.c.v;
}

interface Num<T> {
  kind: "num";
  at: T;
}

interface Neg<T> {
  kind: "neg";
  at: T;
  of: Expression<T>;
}

interface Sum<T> {
  kind: "sum";
  at: T;
  left: Expression<T>;
  right: Expression<T>;
}

interface Product<T> {
  kind: "product";
  at: T;
  left: Expression<T>;
  right: Expression<T>;
}

interface Call<T> {
  kind: "call";
  at: T;
  argument: Expression<T>;
}

type Expression<T> = Num<T> | Neg<T> | Sum<T> | Product<T> | Call<T>;

function positions<T>(expression: Expression<T>): T[] {
  return [];
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
  // A conditional with a branch that isn't a literal keeps the interface hint.
  const extra = { v: 5, w: 6 };
  const mixed = unbox(word.length > 0 ? { v: 1 } : extra);
  assert(String(mixed) === "1", "a conditional with a non-literal branch");
  const chain = { red: 1, next: { green: 2, next: { blue: 3, next: { red: 4, next: null } } } };
  const led: number | null = lead(chain);
  const ledLiteral: number | null = lead({
    red: 1,
    next: { green: 2, next: { blue: 3, next: { red: 4, next: { green: 5, next: null } } } },
  });
  assert(led === null && ledLiteral === null, "interfaces that name each other");

  const fromPair = leftmost({ left: { leaf: "a" }, right: { leaf: "b" } });
  const fromLeaf = leftmost({ leaf: 5 });
  assert(fromPair.length === 1 && fromLeaf + 1 === 6, "a member holding another");

  const deepest = innermost({ a: { b: { c: { v: "d" } } } });
  const values = positions({
    kind: "sum",
    at: 0,
    left: { kind: "neg", at: 1, of: { kind: "num", at: 2 } },
    right: {
      kind: "call",
      at: 3,
      argument: { kind: "product", at: 4, left: { kind: "num", at: 5 }, right: { kind: "num", at: 6 } },
    },
  });
  const numbers: number[] = values;
  assert(deepest.length === 1 && numbers.length === 0, "deep and wide");
}
