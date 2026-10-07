// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A1.2_T2.js
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
  for (let index = 0xD800; index <= 0xDBFF; index++) {
    if (throwsUriError(String.fromCharCode(0x0041, index))) {
      continue;
    }
    if (first < 0) {
      first = index;
    }
    errorCount++;
  }
  assert(errorCount === 0, "a high surrogate after `A` must throw URIError; first failure at " + String(first) + ", " + String(errorCount) + " failures");
}
