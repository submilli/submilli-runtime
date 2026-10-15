// SUB-1825: a function's `length` counts its parameters before the first
// optional, defaulted or rest one, and a function keeps it when it flows into
// a wider function type. JavaScript counts an optional `b?`, since TypeScript
// erases the `?`; Submilli stops at it (SUB-1829), so no case here uses one.
function add(a: number, b: number): number {
  return a + b;
}
function greet(name: string, greeting: string = "hi"): string {
  return greeting + name;
}
function main(): void {
  const count = (...xs: number[]): number => xs.length;
  const pick = (a: string, b: number = 1, ...rest: string[]): string => a;
  const none = (): number => 0;
  assert(add.length === 2);
  assert(greet.length === 1);
  assert(count.length === 0 && pick.length === 1 && none.length === 0);
  const wide: (a: number, b: number, c: number) => number = add;
  assert(wide.length === 2);
  const fns: ((a: number, b: number) => number)[] = [add, (x: number): number => x];
  assert(fns.map((f) => f.length).join(",") === "2,1");
  const shape = { area: (w: number, h: number, d: number): number => w * h * d };
  assert(shape.area.length === 3);
  let n = add.length;
  n += 1;
  assert(n === 3);
}
