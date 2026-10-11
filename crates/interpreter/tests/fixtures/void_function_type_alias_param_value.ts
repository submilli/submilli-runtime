type Consume = (value: void) => number;
function main(): void {
  const consume: Consume = (value) => value === undefined ? 1 : 0;
  assert(consume(undefined) === 1, "aliased function type accepts void parameter");
}
