// A conditional with a `void` branch has no value on that path, so it is
// `void` as a whole and refused wherever a value is needed.
// expect-error: cannot bind a `void` value
// expect-error: expected `number`, got `void`
function f(): void {}

function n(): number {
  return 1;
}

function take(x: number): void {}

function main(): void {
  const k: string = ["a"][0];
  const bound = k === "a" ? f() : n();
  take(k === "a" ? n() : f());
}
