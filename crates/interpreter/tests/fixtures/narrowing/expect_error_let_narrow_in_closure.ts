// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// A `let` root does NOT cross a closure boundary: the enclosing body can
// reassign it after the closure is built, so the capture is boxed and the
// closure would read whatever the binding holds at call time. Hoist to a
// `const` first (see `closure_reguards_field_path.ts`).
function main(): void {
  let s: string | null = "hi";
  if (s !== null) {
    const f = (bump: number): number => s.length + bump;
    assert(f(0) === 2);
  }
}
