// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A1.3_T1.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.

const chars: number[] = [0x0000, 0xD7FF, 0xD800, 0xDBFE, 0xDBFF, 0xE000, 0xFFFF];

function throwsUriError(s: string): boolean {
  try {
    encodeURIComponent(s);
  } catch (e) {
    return e instanceof URIError;
  }
  return false;
}

function main(): void {
  let errorCount = 0;
  let first = -1;
  for (let index = 0xD800; index <= 0xDBFF; index++) {
    let res = true;
    for (let indexC = 0; indexC < chars.length; indexC++) {
      if (!throwsUriError(String.fromCharCode(index, chars[indexC]))) {
        res = false;
      }
    }
    if (res) {
      continue;
    }
    if (first < 0) {
      first = index;
    }
    errorCount++;
  }
  assert(errorCount === 0, "a high surrogate before a non-low-surrogate must throw URIError; first failure at " + String(first) + ", " + String(errorCount) + " failures");
}
