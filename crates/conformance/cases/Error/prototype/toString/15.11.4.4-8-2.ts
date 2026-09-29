// test262: test/built-ins/Error/prototype/toString/15.11.4.4-8-2.js
//
// `new Error()` becomes `new Error("")` (the constructor requires a message);
// the Test262Error throws become assertions.

function main(): void {
  const errObj = new Error("");
  errObj.name = "";
  assertSameValue(errObj.name, "", "errObj.name");
  assertSameValue(errObj.toString(), "", "errObj.toString()");
}
