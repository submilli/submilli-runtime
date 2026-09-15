// test262: test/built-ins/BigInt/prototype/toString/radix-err.js
// The original distinguishes RangeError; the port matches the base Error
// only. The `null` radix row is a compile error here (number expected).

function main(): void {
  const radixes: number[] = [0, 1, 37];
  for (const r of radixes) {
    assertThrows((): void => {
      (0n).toString(r);
    }, `0, radix ${r}`);
    assertThrows((): void => {
      (-1n).toString(r);
    }, `-1, radix ${r}`);
    assertThrows((): void => {
      (1n).toString(r);
    }, `1, radix ${r}`);
  }
}
