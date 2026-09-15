// test262: test/built-ins/BigInt/constructor-from-string-syntax-errors.js
// The runtime throws SyntaxError, matching the original; assertThrows matches via the base Error.

function main(): void {
  assertThrows((): void => {
    BigInt("10n");
  }, "BigInt('10n')");

  assertThrows((): void => {
    BigInt("10x");
  }, "BigInt('10x')");

  assertThrows((): void => {
    BigInt("10b");
  }, "BigInt('10b')");

  assertThrows((): void => {
    BigInt("10.5");
  }, "BigInt('10.5')");

  assertThrows((): void => {
    BigInt("0b");
  }, "BigInt('0b')");

  assertThrows((): void => {
    BigInt("-0x1");
  }, "BigInt('-0x1')");

  assertThrows((): void => {
    BigInt("-0XFFab");
  }, "BigInt('-0XFFab')");

  assertThrows((): void => {
    BigInt("0oa");
  }, "BigInt('0oa')");

  assertThrows((): void => {
    BigInt("000 12");
  }, "BigInt('000 12')");

  assertThrows((): void => {
    BigInt("0o");
  }, "BigInt('0o')");

  assertThrows((): void => {
    BigInt("0x");
  }, "BigInt('0x')");

  assertThrows((): void => {
    BigInt("00o");
  }, "BigInt('00o')");

  assertThrows((): void => {
    BigInt("00b");
  }, "BigInt('00b')");

  assertThrows((): void => {
    BigInt("00x");
  }, "BigInt('00x')");
}
