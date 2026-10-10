function nothing(): void {}
function main(): void {
  const consume: (value: void) => number = (value) => value === undefined ? 1 : 0;
  assert(consume(nothing()) === 1, "function type accepts void parameter");
}
