function main(): void {
  const step: number = 2;
  function inc(x: number, by: number = step): number { return x + by; }
  assert(inc(3) === 5, "nested default captures lexical scope");
  assert(inc(3, undefined) === 5, "explicit undefined invokes nested default");
}
