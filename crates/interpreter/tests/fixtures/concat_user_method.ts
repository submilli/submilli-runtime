// The check that `Array#concat` doesn't spread an array into array elements
// applies to the prelude method only: a user `concat` takes what it declares.
class Joiner<T> {
  concat(...items: T[]): T[][] {
    return [items];
  }
}
function main(): void {
  const joined = new Joiner<number[]>().concat([1, 2]);
  assert(joined.length === 1 && joined[0].length === 1 && joined[0][0].join(",") === "1,2");
}
