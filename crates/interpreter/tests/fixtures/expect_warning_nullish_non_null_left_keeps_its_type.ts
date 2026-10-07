// expect-warning: `??` on non-nullable type `false` — right side is unreachable
// When the left side of `??` can't be `null` and the right side fits its base
// type, the result is the left side's type, as in TypeScript.

function main(): void {
  const off = false;
  const stillOff: false = off ?? true;
  assert(stillOff === false, "`a ?? b` is `a` when `a` can't be null");
}
