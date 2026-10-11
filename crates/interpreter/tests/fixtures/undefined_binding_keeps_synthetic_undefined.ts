// A binding named `undefined` doesn't change the `undefined` the compiler
// writes itself: a typed `let` without an initializer and a padded pattern
// still start from the real `undefined`, as in JavaScript, and neither reads
// a later `const undefined` before its initialization.
function read(undefined: number): number {
  const [a, b = 2] = [1];
  let x: number | undefined;
  return a + b + (x === void 0 ? 100 : x);
}
function before(): number {
  let x: number | undefined;
  const [a, b = 2] = [1];
  const undefined = 5;
  return a + b + (x === void 0 ? 100 : x) + undefined;
}
function outer(): number {
  const r = inner();
  const undefined = 3;
  function inner(): number {
    const [a, b = 2] = [1];
    return a + b;
  }
  return r + undefined;
}
function main(): void {
  assert(read(7) === 103, "a parameter named `undefined`");
  assert(before() === 108, "a later `const undefined`");
  assert(outer() === 6, "a nested function called before `const undefined`");
}
