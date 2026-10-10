// A source that is always null or false has no fields to spread (TS2698 in
// TypeScript).
// expect-error: cannot spread `null` into an object literal
function main(): void {
  const none = { a: 1, ...null };
  console.log(none.a);
}
