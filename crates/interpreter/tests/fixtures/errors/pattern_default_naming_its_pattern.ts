// expect-error: expected `number | false`, got `number | boolean`
// A default that names a slot of its own pattern is no context for the
// source's literals: `b = a` reads the pattern's `a`, a `number`, not the
// outer `boolean`, so `false` widens as tsc has it.
const a: boolean = true;

function main(): void {
  {
    let [a = 1, b = a] = [2, false];
    const c: number | false = b;
  }
}
