// A generic function takes its type arguments by position, so it can't be
// used as a generic function type with other type parameters (tsc accepts).
// expect-error: expected `Same`, got `<A, B>(arg0: A, arg1: B) => boolean`
// expect-error-count: 1
type Same = <X>(a: X, b: X) => boolean;

function main(): void {
  const both: Same = <A, B>(a: A, b: B): boolean => a !== undefined && b !== undefined;
}
