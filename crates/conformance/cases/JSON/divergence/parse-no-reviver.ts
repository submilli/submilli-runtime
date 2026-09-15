// Divergence vector, not a test262 port. JSON.parse is type-directed and
// takes no reviver argument — passing one is a compile error.
// expect-error: takes 1 argument, got 2

function main(): void {
  const n: number = JSON.parse("1", (k: string, v: number): number => v) as number;
  assertSameValue(n, 1, "unreachable");
}
