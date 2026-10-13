// tsc tests the operand a condition decides on: the right operand of an `&&`,
// `||` or `??` condition, and each left operand down a `||` chain. A use
// anywhere later in the `&&` chain, or in the guarded code, excuses it.
// expect-error: this condition will always return true since this function is always defined
// expect-error: right operand of `??` is unreachable because the left operand is never nullish
// expect-error: `a` is specified more than once, so this value is always overwritten
// expect-error: optional chain on non-nullable receiver
// expect-error-count: 6
interface Handlers {
  ready: () => boolean;
}

function isReady(): boolean {
  return true;
}

function main(): void {
  const flag = isReady();
  const handlers: Handlers = { ready: isReady };
  let n = 0;
  if (flag && isReady) { n = 1; }
  if (flag || isReady) { n = 2; }
  const later = isReady && flag && isReady();
  if (handlers.ready && flag) { n = handlers?.ready() ? 3 : 4; }
  const grouped = (isReady && flag) && isReady();
  const value = void n ?? 5;
  const base = { a: 1 };
  const spread = { a: 0, ...base, ...base };
  console.log(n, later, grouped, value, spread);
}
