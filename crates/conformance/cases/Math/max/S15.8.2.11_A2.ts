// test262: test/built-ins/Math/max/S15.8.2.11_A2.js

function main(): void {
  assertSameValue(Math.max(NaN), NaN, "NaN");

  const vals: number[] = [-Infinity, -0.000000000000001, -0, 0, 0.000000000000001, Infinity, NaN];

  const args: number[] = [NaN, NaN];
  for (let i = 0; i <= 1; i++) {
    args[i] = NaN;
    for (let j = 0; j < vals.length; j++) {
      args[1 - i] = vals[j];
      assertSameValue(
        Math.max(args[0], args[1]),
        NaN,
        `max(${args[0]}, ${args[1]})`,
      );
    }
  }

  const args3: number[] = [NaN, NaN, NaN];
  let k = 1;
  let l = 2;
  for (let i = 0; i <= 2; i++) {
    args3[i] = NaN;
    if (i === 1) {
      k = 0;
    } else if (i === 2) {
      l = 1;
    }
    for (let j = 0; j < vals.length; j++) {
      for (let jj = 0; jj < vals.length; jj++) {
        args3[k] = vals[j];
        args3[l] = vals[jj];
        assertSameValue(
          Math.max(args3[0], args3[1], args3[2]),
          NaN,
          `max(${args3[0]}, ${args3[1]}, ${args3[2]})`,
        );
      }
    }
  }
}
