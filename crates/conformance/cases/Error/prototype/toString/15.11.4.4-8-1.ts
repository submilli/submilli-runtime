// test262: test/built-ins/Error/prototype/toString/15.11.4.4-8-1.js

function main(): void {
  const errObj = new Error("ErrorMessage");
  errObj.name = "";

  assertSameValue(errObj.toString(), "ErrorMessage", 'errObj.toString()');
}
