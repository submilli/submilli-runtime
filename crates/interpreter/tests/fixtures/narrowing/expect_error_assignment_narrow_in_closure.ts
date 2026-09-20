// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// A later assignment prevents assignment narrowing from crossing the closure.
function main(): void {
  let s: string | null = null;
  s = "assigned";
  const f = (bump: number): number => s.length + bump;
  s = null;
  assert(f(0) === 8);
}
