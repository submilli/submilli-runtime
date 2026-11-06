// expect-error: cannot infer the type of `before` at this read
// expect-error: cannot infer the type of `captured` at this read
// expect-error: expected `number`, got `string`
function main(): void {
  const before = [];
  const alias: number[] = before;
  before.push(1);
  const captured = [];
  captured.push(1);
  const read = (): number[] => captured;
  read();
  let assigned = [];
  assigned = [1];
  assigned.push("bad");
}
