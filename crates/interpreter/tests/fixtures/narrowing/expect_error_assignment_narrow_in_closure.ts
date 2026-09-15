// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// Assignment narrowing rebinds the view to the real slot rather than a fresh
// shadow, so it reaches the closure through a different install path. A `let`
// is still a `let`: it resets at the boundary.
function main(): void {
  let s: string | null = null;
  s = "assigned";
  const f = (bump: number): number => s.length + bump;
  assert(f(0) === 8);
}
