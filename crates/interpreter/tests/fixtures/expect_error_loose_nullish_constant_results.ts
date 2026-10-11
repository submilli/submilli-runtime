// expect-error: in TypeScript this is always `false`; write `false`
// expect-error: in TypeScript this is always `true`; run `tick();` as its own statement, then use `true`
// expect-error-count: 2
// Two nullish sides: `!=` is always false, and a `void` operand's expression
// still has to run.
function tick(): void {}
function main(): void {
  if (undefined != null) {}
  if (void tick() == null) {}
}
