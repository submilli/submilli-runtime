// test262: test/built-ins/decodeURIComponent/throw-URIError.js
// URIError is erased to the base Error (README: no error subclasses); the
// "throws URIError" checks become "throws".

function main(): void {
  assertThrows((): void => {
    decodeURIComponent("%ED%BF%BF");
  }, "#1: %ED%BF%BF (surrogate pair) should throw URIError");
  assertThrows((): void => {
    decodeURIComponent("%C0%AF");
  }, "#2: %C0%AF (overlong encoding) should throw URIError");
  assertThrows((): void => {
    decodeURIComponent("%ED%7F%BF");
  }, "#3: %ED%7F%BF (invalid continuation) should throw URIError");
  assertThrows((): void => {
    decodeURIComponent("%ED%BF");
  }, "#4: %ED%BF (incomplete sequence) should throw URIError");
  assertThrows((): void => {
    decodeURIComponent("%F4%90%80%80");
  }, "#5: %F4%90%80%80 (out-of-range) should throw URIError");
}
