// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// A parameter is mutable and can be reassigned by the enclosing body *after*
// the closure is built — the case `captured_mutators` does not model, since it
// only scans assignments made inside closures. Excluded for the same reason as
// `let`.
function take(x: string | null): number {
  if (x !== null) {
    const f = (bump: number): number => x.length + bump;
    x = null;
    return f(0);
  }
  return 0;
}

function main(): void {
  assert(take("hi") === 2);
}
