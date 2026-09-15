// test262: test/built-ins/String/prototype/matchAll/flags-nonglobal-throws.js
// expect-fail: matchAll with a non-g RegExp should throw a TypeError; it matches the whole string anyway and returns every match
// The Object.defineProperty(regex, "flags", ...) row is dropped (no property
// descriptors); TypeError is erased to the base Error.

function main(): void {
  assertThrows((): void => {
    "".matchAll(/a/);
  });
  assertThrows((): void => {
    "".matchAll(/a/i);
  });
  assertThrows((): void => {
    "".matchAll(/a/m);
  });
  assertThrows((): void => {
    "".matchAll(/a/u);
  });
  assertThrows((): void => {
    "".matchAll(/a/y);
  });
}
