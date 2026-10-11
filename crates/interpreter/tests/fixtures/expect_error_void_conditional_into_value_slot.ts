// expect-error: expected `number`
function f(): void {}

function n(): number {
  return 1;
}

function take(x: number): void {}

function main(): void {
  const k: string = ["a"][0];
  const bound = k === "a" ? f() : n();
  assert(bound === undefined, "conditional void branch is a value");
  take(k === "a" ? n() : f());
}
