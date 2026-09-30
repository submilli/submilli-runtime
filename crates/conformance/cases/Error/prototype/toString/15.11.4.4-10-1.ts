// test262: test/built-ins/Error/prototype/toString/15.11.4.4-10-1.js

function main(): void {
  const errObj = new Error("ErrorMessage");
  errObj.name = "ErrorName";

  assertSameValue(errObj.toString(), "ErrorName: ErrorMessage", 'errObj.toString()');
}
