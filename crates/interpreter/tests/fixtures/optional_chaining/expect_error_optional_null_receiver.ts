// expect-error: cannot read `toString` on a value of type `null`
// A `null`-typed receiver leaves `strip_null` nothing to strip, so the chain
// has no type to dispatch on. It has to be rejected at the member name, not
// carried forward as the error type.
function main(): void {
  const x: null = null;
  const s: string | null = x?.toString();
  assert(s === null, "unreachable");
}
