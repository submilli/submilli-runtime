// test262: test/built-ins/Error/prototype/toString/15.11.4.4-9-1.js
//
// `new Error()` becomes `new Error("")` (the constructor requires a message).

function main(): void {
  const errObj = new Error("");
  errObj.name = "ErrorName";

  assertSameValue(errObj.toString(), "ErrorName", 'errObj.toString()');
}
