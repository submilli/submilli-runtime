// expect-error: cannot call `splice` on tuple
// expect-error: cannot call `shift` on tuple
// expect-error: cannot call `unshift` on tuple
// expect-error: cannot call `fill` on tuple
// expect-error: cannot call `copyWithin` on tuple
// expect-error: cannot call `pop` on tuple
// expect-error: cannot call `reverse` on tuple
// A tuple is an array at runtime and binds a `T[]` parameter, so every method that
// changes an array's length or contents in place has to stay rejected on one. `push` and
// `sort` have their own fixtures.
function main(): void {
  const pair: [number, number] = [1, 2];
  pair.splice(0, 1);
  pair.shift();
  pair.unshift(0);
  pair.fill(0);
  pair.copyWithin(0, 1);
  pair.pop();
  pair.reverse();
}
