// expect-error: `h` expects 2 argument(s), got 1
// A type argument the call itself writes doesn't make a parameter omittable:
// TypeScript checks the call's arity before applying it.
function h<T>(a: number, b: T): void {}
function main(): void {
  h<void>(1);
}
