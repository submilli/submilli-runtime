// expect-error: cannot infer the type of `values` at this read
function main(): void {
  const values = [];
  const read = (): number[] => values;
  values.push(1);
  read();
}
