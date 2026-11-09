// expect-error: the left-hand side of `for…in` cannot be a destructuring pattern
// expect-error-count: 6
function main(): void {
  for (let [a] in []) {
    const [local] = [1];
    for (const [b] in {}) {
      break;
    }
  }
  for (const {a} in {}) {}
  for (let [a] in [])
    for (const [b] in {}) {}
  for (let [a] in []) {}
  const [after] = [1];
  assert(after === 1, "the enclosing function is preserved");
}
