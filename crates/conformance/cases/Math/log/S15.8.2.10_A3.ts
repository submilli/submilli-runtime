// test262: test/built-ins/Math/log/S15.8.2.10_A3.js

function main(): void {
  assertSameValue(Math.log(0), -Infinity, "Math.log(+0) must return -Infinity");
  assertSameValue(Math.log(-0), -Infinity, "Math.log(-0) must return -Infinity");
}
