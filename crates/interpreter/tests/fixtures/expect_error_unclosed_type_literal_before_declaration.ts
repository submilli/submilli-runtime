// expect-error-count: 1
// expect-error: expected `}`
// An unclosed type literal stops at the next declaration, so `main` is still found.
type A = { a: number
function main(): void {}
