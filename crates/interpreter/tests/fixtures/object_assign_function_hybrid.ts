// `Object.assign` onto a function makes a hybrid, `F & P`: callable as the
// function and holding the properties, as in TypeScript. Unlike JavaScript,
// which adds the properties to the function itself, the hybrid is a new value
// and the function is left as it was; and a slot of a function type holds the
// function a hybrid runs, so `h as (n: number) => number` is not `h`.
interface Tagged {
  (n: number): number;
  label: string;
}
interface WithMethod {
  m(n: number): number;
}
class Loud {
  constructor(public toString: () => string) {}
}
class Point {
  constructor(public x: number) {}
  norm(): number {
    return Math.abs(this.x);
  }
}
class Holder {
  n: number = 1;
  constructor(public t: Tagged) {}
}
function apply(f: (n: number) => number, v: number): number {
  return f(v);
}
function id<T>(x: T): T {
  return x;
}
function main(): void {
  const inc = (n: number): number => n + 1;
  const h = Object.assign(inc, { label: "x" });
  const label: string = h.label;
  assert(h(1) === 2 && label === "x");
  assert(typeof h === "function" && typeof inc === "function");
  assert(h.length === 1);

  const t: Tagged = h;
  assert(t(2) === 3 && t.label.length === 1);
  assert(apply(t, 3) === 4 && apply(h, 4) === 5);
  const g: (n: number) => number = t;
  assert(g(5) === 6 && g.length === 1);
  assert(id(t)(6) === 7 && id(t).label === label);
  const all: Tagged[] = [t];
  assert(all[0]!(7) === 8);

  // Properties: writes, keys, JSON and spread see the object side.
  t.label = "y";
  assert(h.label === "y");
  assert(Object.keys(h).length === 1 && Object.keys(h)[0] === "label");
  assert(JSON.stringify(h) === undefined);
  assert(JSON.stringify({ h, n: 1 }) === '{"n":1}');
  const copy = { ...h };
  assert(copy.label === "y");

  // Assigning onto a hybrid keeps it.
  const more = Object.assign(h, { count: 2 });
  assert(more === h && more.count === 2 && more(1) === 2);

  // Containers, identity and method slots treat it as the function it is.
  assert(JSON.stringify([h]) === "[null]");
  const holder = new Holder(t);
  assert(JSON.stringify(holder) === '{"n":1}');
  const byFunction = new Map<Tagged, number>([[t, 1]]);
  assert(byFunction.get(t) === 1 && new Set<Tagged>([t, t]).size === 1);
  const methods: WithMethod = { m: t };
  assert(methods.m(1) === 2);
  const maybe: Tagged | undefined = label.length > 0 ? h : undefined;
  assert(maybe !== undefined && maybe(2) === 3);

  // A hybrid stands for a conversion method.
  const greeting = Object.assign((): string => "hi", { tone: "warm" });
  const greeter = { toString: greeting };
  assert(`${greeter}` === "hi");

  const loud = new Loud(greeting);
  const anyGreeter: unknown = greeter;
  assert(`${loud}` === "hi" && String(anyGreeter) === "hi");

  // Host functions call a hybrid as the function it holds.
  const descending = Object.assign((a: number, b: number): number => b - a, { kind: "desc" });
  assert([1, 3, 2].sort(descending).join(",") === "3,2,1");
  let seen = 0;
  new Map<string, number>([["k", 4]]).forEach(Object.assign((v: number): void => {
    seen = v;
  }, { kind: "each" }));
  assert(seen === 4);
  const shout = Object.assign((m: string): string => m.toUpperCase(), { kind: "upper" });
  assert("abc".replace(/b/, shout) === "aBc");

  // A field of an intersection-typed hybrid is writable.
  const counter = Object.assign((): number => 0, { n: 1 });
  counter.n = 2;
  counter.n += 1;
  counter.n++;
  assert(counter.n === 4);

  // A nullable hybrid narrows to the hybrid, and an optional chain reads it.
  const found = [counter].find(() => true);
  assert(found?.n === 4 && found?.() === 0);
  if (found !== undefined) {
    found.n = 5;
    assert(found.n === 5 && found() === 0);
  }

  // A union of hybrids reads and calls through each.
  const first = Object.assign((): number => 1, { a: 1, x: "first" });
  const second = Object.assign((): number => 2, { a: 10, y: true });
  const both = [first, second];
  assert(both.map((each) => each() + each.a).join(",") === "2,12");
  let picked: typeof first | typeof second = first;
  if (second.a > 5) {
    picked = second;
  }
  assert(picked() === 2 && picked.a === 10);

  // A class instance's fields copy onto a function; its methods stay behind.
  const fromInstance = Object.assign((): number => 1, new Point(5));
  assert(fromInstance.x === 5 && fromInstance() === 1);

  // Assigning onto an interface-typed hybrid keeps its members.
  const counted = Object.assign(t, { count: 3 });
  assert(counted.label === "y" && counted.count === 3 && counted(3) === 4);

  // Narrowing by `typeof` keeps it on the function side.
  const mixed = label.length > 5 ? "text" : h;
  if (typeof mixed === "function") {
    assert(mixed(1) === 2 && mixed.label === "y");
  } else {
    assert(false);
  }
  const either: Tagged | string = t;
  if (typeof either === "function") {
    assert(either(10) === 11);
  } else {
    assert(false);
  }

  // A function and an object written to one property: the later source's
  // value wins, and an optional later source may leave the earlier one.
  const replaced = Object.assign({ f: t }, { f: { tag: "a" } });
  assert(replaced.f.tag === "a");
  const absent: { f?: { x: number } } = {};
  const kept = Object.assign({}, { f: (n: number): number => n * 2 }, absent).f;
  if (typeof kept === "function") {
    assert(kept(4) === 8);
  } else {
    assert(false);
  }
  const present = { f: { x: 7 } } as { f?: { x: number } };
  const over = Object.assign({}, { f: (n: number): number => n }, present).f;
  if (typeof over === "function") {
    assert(false);
  } else {
    assert(over.x === 7);
  }
}
