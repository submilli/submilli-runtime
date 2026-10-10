function main(): void {
  const consume = (value: void): number => value === undefined ? 1 : 0;
  assert(consume(undefined) === 1, "closure accepts void parameter");
}
