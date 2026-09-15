// expect-error: cannot assert non-null on a value of type `void`
// `!` has to reject a `void` operand rather than pass it through: `void` is a
// return type, not a value, so there is no null to rule out — and admitting it
// reaches `emit_box`, which has no lowering for `void`. Both spellings below
// reach the same check, the chain one through `ChainPart::NonNull`.
class Runner {
  run(): void {}
}

function bare(): void {}

function main(): void {
  const r: Runner | null = new Runner();
  r?.run()!;
  bare()!;
}
