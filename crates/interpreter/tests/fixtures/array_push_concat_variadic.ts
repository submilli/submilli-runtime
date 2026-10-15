// SUB-525: `push` takes any number of elements and `concat` takes elements as
// well as arrays, as tsc and JavaScript do.
function main(): void {
  const xs: number[] = [1];
  assert(xs.push(2, 3, 4) === 4);
  assert(xs.push() === 4);
  assert(xs.join(",") === "1,2,3,4");

  const ys: number[] = xs.concat(5, [6, 7], 8);
  assert(ys.join(",") === "1,2,3,4,5,6,7,8");
  assert(xs.length === 4);

  // An array argument contributes its elements, one level deep.
  const nested: number[][] = [[1]];
  assert(JSON.stringify(nested.concat([2], [[3]])) === "[[1],2,[3]]");

  const table: readonly number[] = [9];
  assert(xs.concat(table).length === 5);
}
