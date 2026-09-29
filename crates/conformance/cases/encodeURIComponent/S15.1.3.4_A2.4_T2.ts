// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A2.4_T2.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.
// Bitwise operators are not in the language (spec.md: deferred); the
// `&` / `>>` byte arithmetic is spelled with Math.floor and %.

const chars: number[] = [0xD800, 0xDBFF, 0xD9FF];

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
  for (let index = 0xDC00; index <= 0xDFFF; index++) {
    let res = true;
    for (let indexC = 0; indexC < chars.length; indexC++) {
      // Math.floor of an integer is a no-op; arithmetic on a value read
      // straight from an array runs far slower in the runtime.
      const char = Math.floor(chars[indexC]);
      const index1 = (char - 0xD800) * 0x400 + (index - 0xDC00) + 0x10000;
      const hex1Byte = 0x0080 + index1 % 0x40;
      const hex1 = "%" + HEX.charAt(Math.floor(hex1Byte / 16) % 16) + HEX.charAt(hex1Byte % 16);
      const hex2Byte = 0x0080 + Math.floor(index1 / 0x40) % 0x40;
      const hex2 = "%" + HEX.charAt(Math.floor(hex2Byte / 16) % 16) + HEX.charAt(hex2Byte % 16);
      const hex3Byte = 0x0080 + Math.floor(index1 / 0x1000) % 0x40;
      const hex3 = "%" + HEX.charAt(Math.floor(hex3Byte / 16) % 16) + HEX.charAt(hex3Byte % 16);
      const hex4Byte = 0x00F0 + Math.floor(index1 / 0x40000) % 0x08;
      const hex4 = "%" + HEX.charAt(Math.floor(hex4Byte / 16) % 16) + HEX.charAt(hex4Byte % 16);
      const str = String.fromCharCode(char, index);
      if (encodeURIComponent(str).toUpperCase() === hex4 + hex3 + hex2 + hex1) {
        continue;
      }
      res = false;
    }
    if (res !== true) {
      if (first < 0) {
        first = index;
      }
      errorCount++;
    }
    count++;
  }
  assert(
    errorCount === 0,
    "#" + decimalToHexString(first) + ": total error " + String(errorCount) + " bad Unicode character in " + String(count),
  );
}
