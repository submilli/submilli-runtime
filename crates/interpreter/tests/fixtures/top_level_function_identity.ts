// Every read of a top-level function as a value gives the same function, as
// in JavaScript: `f === f` holds, and identity-based lookups (`indexOf`,
// `includes`, `Set`, `Map` keys) find it, whether it was read in a function,
// at module level, through a return value, or at a wider function type.
// Identity across a package boundary is tracked separately (SUB-1366).

type Unary = (a: number) => number;
type Binary = (a: number, b: number) => number;

function id(a: number): number {
  return a;
}

function fact(n: number): number {
  return n <= 1 ? 1 : n * fact(n - 1);
}

function withDefault(a: number, b: number = 10): number {
  return a + b;
}

function count(...xs: number[]): number {
  return xs.length;
}

class Counter {
  static next(a: number): number {
    return a + 1;
  }
}

function getFact(): Unary {
  return fact;
}

const atModuleLevel: Unary = id;

function show(label: string, actual: string, expected: string): void {
  console.log(label, actual);
  assert(actual === expected, label);
}

function main(): void {
  const a = id;
  const b = id;
  const typed: Unary = id;
  show("two reads", String(a === b), "true");
  show("typed read", String(typed === id), "true");
  show("module level", String(atModuleLevel === id), "true");
  show("returned", String(getFact() === getFact()), "true");
  show("still callable", String(getFact()(5)), "120");
  const wider: Binary = id;
  const wider2: Binary = id;
  show("wider type", String(wider === wider2), "true");
  show("different functions", String(a === (fact as Unary)), "false");

  show("includes", String([a].includes(id)), "true");
  show("indexOf", String([fact, a].indexOf(id)), "1");
  show("Set size", String(new Set<Unary>([id, fact, b, getFact()]).size), "2");
  const names = new Map<Unary, string>();
  names.set(id, "id");
  show("Map key", names.get(a) ?? "missing", "id");
  const holder = { f: id };
  show("through a field", String(holder.f === id), "true");

  const d1 = withDefault;
  const d2 = withDefault;
  show("default parameter", String(d1 === d2), "true");
  show("default parameter call", String(d1(1, 2)), "3");
  const r1 = count;
  const r2 = count;
  show("rest parameter", String(r1 === r2), "true");
  show("rest parameter call", String(r1(1, 2, 3)), "3");
  show("static method", String(Counter.next === Counter.next), "true");
}
