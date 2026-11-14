// Reordered type parameters would hand the callee its type arguments in the
// wrong order (tsc accepts).
// expect-error: expected `D`, got `S`
// expect-error-count: 1
type S = <A, B>(k1: string, k2: string) => [A | undefined, B | undefined];
type D = <P, Q>(k1: string, k2: string) => [Q | undefined, P | undefined];

function main(): void {
  const s: S = <A, B>(k1: string, k2: string): [A | undefined, B | undefined] => [undefined, undefined];
  const d: D = s;
}
