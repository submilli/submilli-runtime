// expect-error: expected `T`, got `null | T`
// `null` is comparable to a type parameter, so equality with a `T` keeps it,
// as in TypeScript.
function f<T>(x: T | null, y: T): T | string {
  if (x === y) {
    const z: T = x;
    return z;
  }
  return "ne";
}
function main(): void { console.log(f<number>(1, 1)); }
