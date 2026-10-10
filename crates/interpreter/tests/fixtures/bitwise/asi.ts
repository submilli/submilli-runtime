function main(): void {
  let a = 1
  ~a;
  assert(a === 1, "prefix begins a separate statement");
  const b = ~{valueOf: (): number => 3};
  assert(b === -4, "object after prefix");
  const c = ~
    {
      valueOf: (): number => 7
    }
  assert(c === -8, "multiline object after prefix");
}
