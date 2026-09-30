// test262: test/built-ins/decodeURI/S15.1.3.1_A1.15_T1.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.
// URIError is erased to the base Error (README: no error subclasses); the
// "throws URIError" checks become "throws".

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
  for (let indexB = 0xF0; indexB <= 0xF7; indexB++) {
    count++;
    const hexB = "%" + HEX.charAt(Math.floor(indexB / 16) % 16) + HEX.charAt(indexB % 16);
    let result = true;
    for (let indexC = 0x00; indexC <= 0x7F; indexC++) {
      const hexC = "%" + HEX.charAt(Math.floor(indexC / 16) % 16) + HEX.charAt(indexC % 16);
      let threw = false;
      try {
        decodeURI(hexB + hexC + "%A0%A0");
      } catch (e: Error) {
        threw = true;
      }
      if (threw) {
        continue;
      }
      result = false;
    }
    if (result !== true) {
      if (first < 0) {
        first = indexB;
      }
      errorCount++;
    }
  }
  assert(
    errorCount === 0,
    "#" + decimalToHexString(first) + ": total error " + String(errorCount) + " bad Unicode character in " + String(count),
  );
}
