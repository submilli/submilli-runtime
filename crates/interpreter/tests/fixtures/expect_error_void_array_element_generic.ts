// `Array<T>` desugars to `T[]` through a separate arm, so it needs its own
// screen and its own fixture.
// expect-error: `void` cannot be an array element — it has no values
function main(): void {
  const generic: Array<void> = [];
}
