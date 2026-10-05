// A `?.` on a receiver that can't be `null` never short-circuits, so it adds
// no `| null` to the chain's type, as in tsc. Only a step whose receiver can be
// `null` does, and a type parameter can be: a caller may instantiate it with
// `null`.
interface Inner {
  d: number;
}

interface Outer {
  b: string;
  c: Inner | null;
}

function readB(o: Outer): string {
  return o?.b;
}

function readD(o: Outer): number | null {
  return o?.c?.d ?? null;
}

function readAsserted(o: Outer): number {
  return o?.c!.d;
}

function lengthOf(s: string): number {
  return s?.length;
}

function firstLabel<T>(items: T[]): string {
  return items[0]?.toString() ?? "none";
}

function main(): void {
  const full: Outer = { b: "hi", c: { d: 3 } };
  const empty: Outer = { b: "bye", c: null };
  console.log(readB(full), readD(full), readAsserted(full), lengthOf("abc"));
  console.log(readB(empty), readD(empty));
  const missing: (number | null)[] = [null];
  console.log(firstLabel(missing), firstLabel([7]));
}
