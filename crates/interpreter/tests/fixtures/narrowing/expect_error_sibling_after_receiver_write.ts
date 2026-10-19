// expect-error: expected `number`, got `number | string`
// expect-error-count: 1
// A variable read off a receiver that is written afterwards doesn't narrow its
// siblings: the receiver may hold another value by the later read.
type Entry = [true, number] | [false, string];
function read(first: Entry, next: Entry): string {
  let entry = first;
  const ok = entry[0];
  entry = next;
  const value = entry[1];
  if (ok) {
    const count: number = value;
    return count.toFixed(0);
  }
  return "";
}
function main(): void {
  read([true, 1], [false, "x"]);
}
