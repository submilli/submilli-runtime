// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// A guard cannot cross a closure when the enclosing body later reassigns it.
function main(): void {
  let s: string | null = "hi";
  if (s !== null) {
    const f = (bump: number): number => s.length + bump;
    s = null;
    assert(f(0) === 2);
  }
}
