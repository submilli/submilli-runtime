// Arguments are checked in order: `mk(s)` runs before the next argument
// assigns `s`, so `s` is still `null` there.
// expect-error: expected `string`, got `null`
// expect-error-count: 1
function callWith<T>(f: (t: T) => number, t: T): number {
  return f(t);
}
function mk(s: string): (n: number) => number {
  return (n) => n + s.length;
}
function main(): void {
  let s: string | null = null;
  console.log(callWith(mk(s), (s = "xyz").length));
}
