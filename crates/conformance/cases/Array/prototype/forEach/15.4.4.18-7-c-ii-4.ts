// test262: test/built-ins/Array/prototype/forEach/15.4.4.18-7-c-ii-4.js
// Adapted: ascending order is observed through the element values (equal to
// their indices) since our callbacks receive only the element.

function main(): void {
  const arr = [0, 1, 2, 3, 4, 5];
  let lastIdx = 0;
  let called = 0;
  let result = true;

  const callbackfn = (val: number): void => {
    called++;
    if (lastIdx !== val) {
      result = false;
    } else {
      lastIdx++;
    }
  };

  arr.forEach(callbackfn);

  assert(result, "result !== true");
  assertSameValue(arr.length, called, "arr.length");
}
