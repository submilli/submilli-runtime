// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A2.4_T1.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.
// Bitwise operators are not in the language (spec.md: deferred); the
// `&` / `>>` byte arithmetic is spelled with Math.floor and %.

// test262 harness helpers (harness/decimalToHexString.js).
const HEX: string = "0123456789ABCDEF";

function decimalToHexString(n: number): string {
  let s = "";
  let v = n;
  while (v > 0) {
    s = HEX.charAt(v % 16) + s;
    v = Math.floor(v / 16);
  }
  while (s.length < 4) {
    s = "0" + s;
  }
  return s;
}

// decimalToPercentHexString(n) is inlined at its call sites as
// "%" + HEX.charAt(Math.floor(n / 16) % 16) + HEX.charAt(n % 16): a user-function
// call costs more than the rest of the loop body here.

function main(): void {
  let errorCount = 0;
  let count = 0;
  let first = -1;
  for (let indexB1 = 0xE0; indexB1 <= 0xEF; indexB1++) {
    const hexB1 = "%" + HEX.charAt(Math.floor(indexB1 / 16) % 16) + HEX.charAt(indexB1 % 16);
    for (let indexB2 = 0x80; indexB2 <= 0xBF; indexB2++) {
      if ((indexB1 === 0xE0) && (indexB2 <= 0x9F)) {
        continue;
      }
      if ((indexB1 === 0xED) && (0xA0 <= indexB2)) {
        continue;
      }
      const hexB1_B2 = hexB1 + "%" + HEX.charAt(Math.floor(indexB2 / 16) % 16) + HEX.charAt(indexB2 % 16);
      for (let indexB3 = 0x80; indexB3 <= 0xBF; indexB3++) {
        count++;
        const hexB1_B2_B3 = hexB1_B2 + "%" + HEX.charAt(Math.floor(indexB3 / 16) % 16) + HEX.charAt(indexB3 % 16);
        const index = (indexB1 % 0x10) * 0x1000 + (indexB2 % 0x40) * 0x40 + indexB3 % 0x40;
        if (decodeURIComponent(hexB1_B2_B3) === String.fromCharCode(index)) {
          continue;
        }
        if (first < 0) {
          first = index;
        }
        errorCount++;
      }
    }
  }
  assert(
    errorCount === 0,
    "#" + decimalToHexString(first) + ": total error " + String(errorCount) + " bad Unicode character in " + String(count),
  );
}
