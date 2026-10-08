// Elements no single member of the union fits are rejected, as tsc does; an
// element that fits no member's element type is reported once, by itself.
// expect-error: expected `number[] | string[]`, got `(number | string)[]`
// expect-error: expected `1 | 2 | "x" | "y"`, got `3`
// expect-error-count: 2
function main(): void {
  const mixed: number[] | string[] = [1, "a"];
  const outside: (1 | 2)[] | ("x" | "y")[] = [1, 3];
}
