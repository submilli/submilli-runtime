// test262: test/built-ins/Math/pow/applying-the-exp-operator_A23.js

function main(): void {
  const base: number[] = [
    -1.7976931348623157e308,
    -Math.PI,
    -1,
    -0.000000000000001,
  ];
  const exponent: number[] = [
    -Math.PI,
    -Math.E,
    -1.000000000000001,
    -0.000000000000001,
    0.000000000000001,
    1.000000000000001,
    Math.E,
    Math.PI,
  ];

  for (let i = 0; i < base.length; i++) {
    for (let j = 0; j < exponent.length; j++) {
      assertSameValue(
        Math.pow(base[i], exponent[j]),
        NaN,
        `(${base[i]}, ${exponent[j]})`,
      );
    }
  }
}
