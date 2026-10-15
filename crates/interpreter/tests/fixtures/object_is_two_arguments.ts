// SUB-1761: Object.is takes two required arguments, as in tsc; `undefined`
// is passed explicitly.
function main(): void {
  assert(Object.is(NaN, NaN) && !Object.is(0, -0));
  assert(Object.is(undefined, undefined) && !Object.is(undefined, null));
  assert(Object.is("a", "a"));
}
