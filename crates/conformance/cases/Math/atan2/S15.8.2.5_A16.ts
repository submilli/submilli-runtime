// test262: test/built-ins/Math/atan2/S15.8.2.5_A16.js

function main(): void {
  const x: number = Infinity;
  const y: number[] = [-0.000000000000001, -1, -1.7976931348623157e308];

  for (let i = 0; i < y.length; i++) {
    assertSameValue(
      Math.atan2(y[i], x),
      -0,
      `(${y[i]}, Infinity)`,
    );
  }
}
