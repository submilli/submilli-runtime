// expect-error: function f(a: number | undefined, b: number): number
// expect-error: C.m(a: number | undefined, b: string): string
// expect-error-count: 2
// A defaulted parameter a required one follows can't print as `a?`, which
// TypeScript would reject; it prints with `undefined`, as tsc shows it.
function f(a: number = 1, b: number): number {
  return a + b;
}
class C {
  m(a: number = 1, b: string): string {
    return `${a}${b}`;
  }
}
function main(): void {
  f(2);
  new C().m(1);
}
