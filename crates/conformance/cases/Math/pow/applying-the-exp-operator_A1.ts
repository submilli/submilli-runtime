// test262: test/built-ins/Math/pow/applying-the-exp-operator_A1.js
// expect-fail: Math.pow(1, NaN) returns 1 (IEEE 754 pow via the host's powf); the standard returns NaN whenever the exponent is NaN, even for base 1

function main(): void {
  const exponent: number = NaN;
  const base: number[] = [
    -Infinity,
    -1.7976931348623157e308,
    -0.000000000000001,
    -0,
    0,
    0.000000000000001,
    1.7976931348623157e308,
    Infinity,
    NaN,
    1,
  ];

  for (let i = 0; i < base.length; i++) {
    assertSameValue(Math.pow(base[i], exponent), NaN, `${base[i]}`);
  }
}
