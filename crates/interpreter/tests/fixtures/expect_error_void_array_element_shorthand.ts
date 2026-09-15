// expect-error: `void` cannot be an array element — it has no values
function main(): void {
  const shorthand: void[] = [];
}
