// test262: test/built-ins/Object/keys/15.2.3.14-1-3.js
// Object.keys does not throw on a non-object first argument (string).
// JS would enumerate the string's index keys; here non-objects yield `[]`
// (see divergence/values-erased-to-unknown.ts) — the no-throw intent is
// what this case pins.

function main(): void {
  Object.keys("abc");
}
