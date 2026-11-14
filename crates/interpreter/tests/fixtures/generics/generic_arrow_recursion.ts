// A generic `const` arrow calls itself: its early type, from its written
// signature, carries its own type parameters.
const repeatLast = <T,>(n: number, v: T): T => n <= 0 ? v : repeatLast(n - 1, v);

function main(): void {
  assert(repeatLast(3, "x") === "x", "a string through recursion");
  assert(repeatLast<number>(2, 7) === 7, "explicit type arguments");
}
