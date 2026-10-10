// expect-error-count: 1
// expect-error: `this` is only valid inside a class method or constructor body
function describe(): string {
  return this.name;
}

function main(): void {}
