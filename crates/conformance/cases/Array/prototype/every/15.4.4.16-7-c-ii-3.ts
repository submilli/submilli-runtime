// test262: test/built-ins/Array/prototype/every/15.4.4.16-7-c-ii-3.js
// Adapted: the callback decides by element value instead of index — the
// array's values equal their indices, so the short-circuit point is the same.

function main(): void {
  let callCnt = 0;

  const callbackfn = (val: number): boolean => {
    callCnt++;
    if (val > 5) {
      return false;
    }
    return true;
  };

  const arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

  assertSameValue(arr.every(callbackfn), false, "arr.every(callbackfn)");
  assertSameValue(callCnt, 7, "callCnt");
}
