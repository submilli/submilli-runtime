// SUB-1762: writing an array's length truncates it, as in JavaScript. Growing
// it would leave holes, which Submilli arrays cannot hold, so that throws a
// RangeError. A readonly array's or a tuple's length stays read-only.
function lengthOf(xs: readonly unknown[]): number {
  return xs.length;
}
function main(): void {
  const xs = [1, 2, 3, 4, 5];
  xs.length = 3;
  assert(xs.join(",") === "1,2,3");
  xs.length -= 1;
  assert(xs.join(",") === "1,2");
  xs.length--;
  assert(xs.join(",") === "1");
  const before = xs.length--;
  assert(before === 1 && lengthOf(xs) === 0);
  const ys = ["a", "b"];
  ys.length = 2;
  assert(ys.join(",") === "a,b");
  const cleared = (ys.length = 0);
  assert(cleared === 0 && lengthOf(ys) === 0);
  ys.push("c");
  assert(ys[0] === "c" && lengthOf(ys) === 1);
  let grew = false;
  try {
    ys.length = 5;
  } catch (e) {
    grew = e instanceof RangeError;
  }
  assert(grew && lengthOf(ys) === 1);
  let fractional = false;
  try {
    ys.length = 0.5;
  } catch (e) {
    fractional = e instanceof RangeError;
  }
  assert(fractional && lengthOf(ys) === 1);
}
