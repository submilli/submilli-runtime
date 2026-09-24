function replace(values: unknown[]): void { values[0] = "changed"; }
function main(): true {
  const values: unknown[] = [true];
  if (values[0] === true) {
    replace(values);
    return values[0];
  }
  return true;
}
