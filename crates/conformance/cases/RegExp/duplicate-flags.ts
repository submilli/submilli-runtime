// test262: test/built-ins/RegExp/duplicate-flags.js
// The d-flag rows are dropped (no d flag); SyntaxError is erased to the base Error.

function main(): void {
  const g = new RegExp("", "mig");
  assert(g.global, "single g is accepted");
  assertThrows((): void => {
    new RegExp("", "migg");
  }, "duplicate g");
  assertThrows((): void => {
    new RegExp("", "ii");
  }, "duplicate i");
  assertThrows((): void => {
    new RegExp("", "mm");
  }, "duplicate m");
  assertThrows((): void => {
    new RegExp("", "ss");
  }, "duplicate s");
  assertThrows((): void => {
    new RegExp("", "uu");
  }, "duplicate u");
  assertThrows((): void => {
    new RegExp("", "yy");
  }, "duplicate y");
}
