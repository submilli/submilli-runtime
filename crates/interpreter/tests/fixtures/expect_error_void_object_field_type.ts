// expect-error: `void` cannot be a field type — it has no values
function main(): void {
  const obj: { v: void } = { v: nothing() };
}

function nothing(): void {}
