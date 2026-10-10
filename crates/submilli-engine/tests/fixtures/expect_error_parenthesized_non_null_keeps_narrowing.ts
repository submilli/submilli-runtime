// Only a reference right under `!` reads at its declared type, as in
// TypeScript: `(b.v)!` keeps the guard's `null`, so `!` leaves `never`.
// expect-error: cannot read field `length` on non-object type `never`
// expect-error-count: 1
type Box = { v: string | null };
function main(): void {
  const b: Box = { v: null };
  if (b.v === null) {
    console.log((b.v)!.length);
  }
}
