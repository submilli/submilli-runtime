// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A2.2_T1.js
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
  for (let index = 0x0080; index <= 0x07FF; index++) {
    count++;
    const hex1Byte = 0x0080 + index % 0x40;
    const hex1 = "%" + HEX.charAt(Math.floor(hex1Byte / 16) % 16) + HEX.charAt(hex1Byte % 16);
    const hex2Byte = 0x00C0 + Math.floor(index / 0x40) % 0x20;
    const hex2 = "%" + HEX.charAt(Math.floor(hex2Byte / 16) % 16) + HEX.charAt(hex2Byte % 16);
    const str = String.fromCharCode(index);
    if (encodeURIComponent(str).toUpperCase() === hex2 + hex1) {
      continue;
    }
    if (first < 0) {
      first = index;
    }
    errorCount++;
  }
  assert(
    errorCount === 0,
    "#" + decimalToHexString(first) + ": total error " + String(errorCount) + " bad Unicode character in " + String(count),
  );
}
