// An optional field declared `null` doesn't fit an optional field of another
// type (TS2322 in TypeScript); only the fields array-literal normalization
// adds for names other elements write do.
// expect-error: expected `T`, got `S`
type S = { a?: null; b: number };
type T = { a?: number; b: number };

function main(): void {
  const s: S = { b: 1 };
  const t: T = s;
  console.log(t.b);
}
