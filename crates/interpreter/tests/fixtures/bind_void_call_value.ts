function noop(): void {}
function main(): void {
  const value = noop();
  assert(value === undefined, "binding a void call yields undefined");
}
