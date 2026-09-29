// test262: test/built-ins/encodeURI/S15.1.3.3_A2.1_T1.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.

const uriUnescaped: string[] = ["-", "_", ".", "!", "~", "*", "'", "(", ")", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const uriReserved: string[] = [";", "/", "?", ":", "@", "&", "=", "+", "$", ","];

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

function decimalToPercentHexString(n: number): string {
  return "%" + HEX.charAt(Math.floor(n / 16) % 16) + HEX.charAt(n % 16);
}

// The labeled `continue l` becomes an early return from this helper.
function encodesCorrectly(index: number): boolean {
  const str = String.fromCharCode(index);
  for (let indexC = 0; indexC < uriReserved.length; indexC++) {
    if (uriReserved[indexC] === str) {
      return true;
    }
  }
  for (let indexC = 0; indexC < uriUnescaped.length; indexC++) {
    if (uriUnescaped[indexC] === str) {
      return true;
    }
  }
  if ("#" === str) {
    return true;
  }
  return encodeURI(str).toUpperCase() === decimalToPercentHexString(index);
}

function main(): void {
  let errorCount = 0;
  let count = 0;
  let first = -1;
  for (let index = 0x0000; index <= 0x007F; index++) {
    count++;
    if (encodesCorrectly(index)) {
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
