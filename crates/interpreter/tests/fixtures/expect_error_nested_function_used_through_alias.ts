// Taking `a` as a value counts as a use: the value could be called before `c`
// exists, through `b`. (Here it is called only afterwards, which JavaScript
// would run; this is deliberately conservative.)
// expect-error: `a` is used before `c`, which `b` uses, is declared
// expect-error-count: 1
function main(): void {
  const f = a;
  const c = 1;
  function a(): number {
    return b();
  }
  function b(): number {
    return c;
  }
  console.log(f());
}
