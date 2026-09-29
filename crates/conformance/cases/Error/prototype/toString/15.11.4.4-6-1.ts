// test262: test/built-ins/Error/prototype/toString/15.11.4.4-6-1.js
//
// The constructor requires a message, so `new Error()` becomes
// `new Error("")`. Both reach the same toString step: an empty message
// yields the name alone.

function main(): void {
  const errObj = new Error("");

  assertSameValue(errObj.toString(), "Error", 'errObj.toString()');
}
