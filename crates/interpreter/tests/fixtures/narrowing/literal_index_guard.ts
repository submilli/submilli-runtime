function single(values: unknown[]): string {
  if (values[0] === "a") { const value: string = values[0]; return value; }
  return "other";
}
function alternatives(values: unknown[]): string {
  if (values[0] === "a" || values[0] === "b") { return values[0]; }
  return "other";
}
function optional(values: unknown[]): string | null | undefined {
  if (values[0] === "a") { return values?.[0]; }
  return null;
}
function writesInvalidate(): void {
  let values: unknown[] = ["a"];
  if (values[0] === "a") {
    values[0] = 3;
    assert(values[0] === 3);
  }
  values = ["a"];
  if (values[0] === "a") {
    values = [false];
    assert(values[0] === false);
  }
}
function numericWritesInvalidate(): void {
  const values: number[] = [1];
  if (values[0] === 1) {
    values[0]++;
    assert(values[0] === 2);
  }
  if (values[0] === 2) {
    values[0] += 1;
    assert(values[0] === 3);
  }
}
function unknownUpdates(): void {
  const values: unknown[] = [1];
  if (values[0] === 1) {
    const previous = values[0]++;
    assert(previous === 1);
    assert(values[0] === 2);
  }
  if (values[0] === 2) {
    values[0] += 1;
    assert(values[0] === 3);
  }
}
function main(): void {
  unknownUpdates();
  writesInvalidate();
  numericWritesInvalidate();
  assert(single(["a"]) === "a");
  assert(single(["b"]) === "other");
  assert(alternatives(["a"]) === "a");
  assert(alternatives(["b"]) === "b");
  assert(alternatives([3]) === "other");
  assert(optional(["a"]) === "a");
  assert(optional([3]) === null);
}
