// test262: test/built-ins/Array/prototype/filter/15.4.4.20-9-c-ii-4.js
// Adapted: ascending order is observed through the element values (equal to
// their indices) since our callbacks receive only the element.

function main(): void {
  const arr = [0, 1, 2, 3, 4, 5];
  let lastIdx = 0;
  let called = 0;

  const callbackfn = (val: number): boolean => {
    called++;
    if (lastIdx !== val) {
      return false;
    }
    lastIdx++;
    return true;
  };

  const newArr = arr.filter(callbackfn);

  assertSameValue(newArr.length, called, "newArr.length");
  assertCompareArray(newArr, arr, "every element visited in ascending order is kept");
}
