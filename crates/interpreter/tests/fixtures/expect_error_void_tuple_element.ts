// expect-error: `void` cannot be a tuple element — it has no values
function main(): void {
  const tup: [number, void] = [1, nothing()];
}

function nothing(): void {}
