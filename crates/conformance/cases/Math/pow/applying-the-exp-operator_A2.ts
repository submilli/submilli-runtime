// test262: test/built-ins/Math/pow/applying-the-exp-operator_A2.js

function main(): void {
  const exponent: number = 0;
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
  ];

  for (let i = 0; i < base.length; i++) {
    assert(
      Math.pow(base[i], exponent) === 1,
      `#1: Math.pow(${base[i]}, ${exponent}) !== 1`,
    );
  }
}
