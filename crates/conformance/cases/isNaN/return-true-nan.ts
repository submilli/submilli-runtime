// test262: test/built-ins/isNaN/return-true-nan.js
//
// The harness NaNs list (distinct NaN bit patterns) is inlined; the
// Number("Not-a-Number") and Math.pow(-1, 0.5) entries are expressed with
// the same operations available here.

function main(): void {
  const NaNs: number[] = [
    NaN,
    Number.NaN,
    NaN * 0,
    0 / 0,
    Infinity / Infinity,
    -(0 / 0),
    Math.pow(-1, 0.5),
    -Math.pow(-1, 0.5),
    Number("Not-a-Number"),
  ];

  for (let i = 0; i < NaNs.length; i++) {
    assertSameValue(isNaN(NaNs[i]), true, "value on position: " + i.toString());
  }
}
