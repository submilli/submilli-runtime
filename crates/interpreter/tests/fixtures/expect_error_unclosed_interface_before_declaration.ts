// expect-error-count: 1
// expect-error: expected `}` to close interface
// An unclosed interface stops at the next declaration, so `main` is still found.
interface I { a: number;
function main(): void {}
