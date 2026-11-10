// expect-error: the left-hand side of `for…in` cannot be a destructuring pattern
// expect-error-count: 4
for (let [a, b] in []) {}
for (let {a, b} in []) {}
for (const [k, v] in { key: 1 }) {}
for (const {k, v} in { key: 1 }) {}
function main(): void {}
