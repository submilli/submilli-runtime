// test262: test/built-ins/Number/EPSILON.js

function main(): void {
  assert(Number.EPSILON > 0, "value is greater than 0");
  assert(Number.EPSILON < 0.000001, "value is smaller than 0.000001");
}
