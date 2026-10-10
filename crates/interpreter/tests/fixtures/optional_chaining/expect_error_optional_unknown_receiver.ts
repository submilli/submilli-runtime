// expect-error: cannot read `toUpperCase` on a value of type `unknown`
// expect-error: cannot read `length` on a value of type `unknown`
// expect-error: cannot index into a value of type `unknown`
// `?.` narrows away `null`, not `unknown`: every member of an un-narrowed
// `unknown` resolves to `unknown` again, which has no member to dispatch on and
// no Wasm lowering to reach.
function main(): void {
  const u: unknown = "hi";
  const called: unknown = u?.toUpperCase();
  const read: unknown = u?.length;
  const indexed: unknown = u?.[0];
  assert(called === read && read === indexed, "unreachable");
}
