let calls = 0;
function radix(): number { calls += 1; return 16; }
function render(value: number | boolean | string | bigint | undefined): string | undefined {
  return value?.toString(radix());
}
function unwrap(value: number | boolean | undefined): number | boolean | undefined {
  return value?.valueOf();
}
function main(): void {
  assert(render(undefined) === undefined);
  assert(calls === 0);
  assert(render(42) === "2a");
  assert(render(false) === "false");
  assert(render("text") === "text");
  assert(render(42n) === "2a");
  assert(calls === 4);
  assert(unwrap(42) === 42);
  assert(unwrap(false) === false);
  assert(unwrap(undefined) === undefined);
}
