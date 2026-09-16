function g(a: number, ...rest: number[]): string { return "top"; }
function generic<T>(a: T): T { return a; }
const twice = (n: number): number => n * 2;
function main(): void {
  const g = (a: number, b: number): string => "local";
  assert(g(1, 2) === "local", "local arity and dispatch");
  assert(twice(3) === 6, "top-level arrow");
  const generic = (n: number): number => n + 1;
  assert(generic(2) === 3, "local shadows generic function");
}
