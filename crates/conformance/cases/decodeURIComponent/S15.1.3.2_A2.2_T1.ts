// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A2.2_T1.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.

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

function decodesCorrectly(indexB1: number): boolean {
  const hexB1 = "%" + HEX.charAt(Math.floor(indexB1 / 16) % 16) + HEX.charAt(indexB1 % 16);
  const hex = String.fromCharCode(indexB1);
  return decodeURIComponent(hexB1) === hex;
}

function main(): void {
  let errorCount = 0;
  let count = 0;
  let first = -1;
  for (let indexB1 = 0x00; indexB1 <= 0x7F; indexB1++) {
    count++;
    if (decodesCorrectly(indexB1)) {
      continue;
    }
    if (first < 0) {
      first = indexB1;
    }
    errorCount++;
  }
  assert(
    errorCount === 0,
    "#" + decimalToHexString(first) + ": total error " + String(errorCount) + " bad Unicode character in " + String(count),
  );
}
