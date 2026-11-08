// Two members whose types only name each other have none (TS2502).
// expect-error: `T[K]` cannot be applied to `J` inside its own definition
// expect-error-count: 1
interface I { a: J["b"] }
interface J { b: I["a"] }
function main(): void {}
