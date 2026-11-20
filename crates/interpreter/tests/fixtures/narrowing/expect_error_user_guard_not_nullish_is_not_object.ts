// expect-error: type-guard return value does not narrow parameter `v` to `Foo`
// Ruling out `null` and `undefined` leaves `{}`, which still holds primitives,
// so the guard isn't left to a run-time check.
interface Foo { a: number }
function isFoo(v: unknown): v is Foo {
  return v !== null && v !== undefined;
}
export function main(): void {}
