// test262: test/built-ins/Math/atan2/S15.8.2.5_A1.js

function main(): void {
  const vals: number[] = [-Infinity, -0.000000000000001, -0, 0, 0.000000000000001, Infinity, NaN];

  const args: number[] = [NaN, NaN];
  for (let i = 0; i < 2; i++) {
    args[i] = NaN;
    for (let j = 0; j < vals.length; j++) {
      args[1 - i] = vals[j];
      assertSameValue(
        Math.atan2(args[0], args[1]),
        NaN,
        `(${args[0]}, ${args[1]})`,
      );
    }
  }
}
