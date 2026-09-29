// Divergence pin (spec.md "URI handling"): a lone surrogate reaches the encoder
// already replaced by U+FFFD, so it encodes as %EF%BF%BD; JS throws URIError.
// Pins the Submilli side of test/built-ins/encodeURI/S15.1.3.3_A1.{1,2,3}_T*.js,
// whose originals live in rejected/encodeURI/.

function main(): void {
  assertSameValue(encodeURI(String.fromCharCode(0xD800)), "%EF%BF%BD", "lone high surrogate (JS: URIError)");
  assertSameValue(encodeURI(String.fromCharCode(0xDFFF)), "%EF%BF%BD", "lone low surrogate (JS: URIError)");
  assertSameValue(encodeURI(String.fromCharCode(0xDC00, 0x0041)), "%EF%BF%BDA", "low surrogate first (JS: URIError)");
  assertSameValue(encodeURI(String.fromCharCode(0x0041, 0xDBFF)), "A%EF%BF%BD", "trailing high surrogate (JS: URIError)");
  assertSameValue(encodeURI(String.fromCharCode(0xD800, 0xE000)), "%EF%BF%BD%EE%80%80", "high surrogate before a non-surrogate (JS: URIError)");
  assertSameValue(encodeURI(String.fromCharCode(0xD800, 0xDC00)), "%F0%90%80%80", "a well-formed pair still encodes as one code point");
}
