// An assertion function renders as its `void` return, which loses the
// assertion the same way a guard's `boolean` rendering loses the guard.
// expect-error: got `(arg0: unknown) => void`
// expect-error: this value is an assertion function (`asserts arg0 is string`), and the rendering above is lossy
// expect-error: Only a direct call of the assertion narrows.
// expect-error: this value is an assertion function (`asserts arg0`)

function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}
function assertPresent(x: string | null): asserts x {
  if (x === null) throw new Error("missing");
}

export function main(): void {
  const bad: number = assertIsString;
  const alsoBad: number = assertPresent;
}
