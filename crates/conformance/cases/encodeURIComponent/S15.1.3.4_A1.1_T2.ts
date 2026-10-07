// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A1.1_T2.js
// The Test262Error range reporting (indexO/indexP) becomes one assert naming
// the first failing value and the failure count.

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
  for (let index = 0xDC00; index <= 0xDFFF; index++) {
    if (throwsUriError(String.fromCharCode(index, 0x0041))) {
      continue;
    }
    if (first < 0) {
      first = index;
    }
    errorCount++;
  }
  assert(errorCount === 0, "a low surrogate before `A` must throw URIError; first failure at " + String(first) + ", " + String(errorCount) + " failures");
}
