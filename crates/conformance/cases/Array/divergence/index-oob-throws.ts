// Not a test262 port: pins a documented divergence (spec.md §1.2). Indexed
// access past the end throws a catchable Error on both read and write — JS
// returns undefined on read and sparse-extends on write; neither is
// representable here. at() is the null-returning accessor.

function main(): void {
  const arr = [10, 20, 30];

  let readThrew = false;
  try {
    const v = arr[3];
    assert(v !== v, "unreachable: OOB read should have thrown");
  } catch (e: Error) {
    readThrew = true;
  }
  assert(readThrew, "OOB read throws a catchable Error");

  let writeThrew = false;
  try {
    arr[10] = 99;
  } catch (e: Error) {
    writeThrew = true;
  }
  assert(writeThrew, "OOB write throws a catchable Error");
  assertSameValue(arr.length, 3, "a failed OOB write does not extend the array");

  assertSameValue(arr.at(3), null, "at() returns null past the end instead of throwing");
}
