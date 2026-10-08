// A union of function types is called through one combined signature, as in
// tsc: each parameter takes what every member accepts there, the result is
// the union of the members' results, and a member with fewer parameters
// ignores the extra arguments.
type A = { x: number; y?: number };
type B = { x: number; z?: number };

function objects(f: ((a: A) => number) | ((b: B) => number)): number {
  return f({ x: 1 });
}

function arities(f: ((a: number) => string) | ((a: number, b: string) => string)): string {
  return f(1, "q");
}

function literals(f: ((a: number) => 1) | ((a: number) => "s")): 1 | "s" {
  return f(3);
}

interface IA {
  x: number;
}

interface IB {
  y: string;
}

function interfaces(f: ((a: IA) => string) | ((b: IB) => string)): string {
  return f({ x: 1, y: "q" });
}

type Either = ((a: number) => number) | ((a: number, b: number) => number);

function callMaybe(maybe: Either | null): number | null {
  return maybe?.(2, 3) ?? null;
}

function effects(f: ((a: string) => void) | (() => void)): void {
  f("w");
}

function main(): void {
  assert(objects((a: A) => a.x) === 1);
  assert(objects((b: B) => b.x + 1) === 2);
  assert(arities((a: number) => "one" + String(a)) === "one1");
  assert(arities((a: number, b: string) => b + String(a)) === "q1");
  assert(literals(() => 1) === 1);
  assert(literals((a: number) => "s") === "s");
  const seen: string[] = [];
  effects((s: string) => { seen.push(s); });
  effects(() => { seen.push("none"); });
  assert(seen.join(",") === "w,none");
  assert(interfaces((a: IA) => String(a.x)) === "1", "interface parameters");
  const box: { cb: Either } | null = { cb: (a: number) => a * 7 };
  assert(box?.cb(2, 0) === 14, "an optional chain to a field");
  assert(callMaybe((a: number, b: number) => a + b) === 5, "an optional call");
  const either: Either[] = [(a: number) => a];
  assert(either.at(0)?.(4, 4) === 4, "an optional call on an element");
  const handlers = [(a: A) => 1, (b: B) => 2];
  assert(handlers[1]({ x: 0 }) === 2);
}
