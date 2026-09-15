// test262: test/built-ins/Boolean/prototype/toString/S15.6.4.2_A1_T1.js
//
// No wrapper objects here: the `Boolean.prototype.toString()` and
// `new Boolean(...)` receivers are distilled to the boolean values they box.

function main(): void {
  assertSameValue(false.toString(), "false", 'false.toString() must return "false"');
  assertSameValue(true.toString(), "true", 'true.toString() must return "true"');
}
