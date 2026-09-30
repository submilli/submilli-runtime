// test262: test/built-ins/Error/prototype/toString/15.11.4.4-6-2.js

function main(): void {
  const errObj = new Error("ErrorMessage");

  assertSameValue(errObj.toString(), "Error: ErrorMessage", 'errObj.toString()');
}
