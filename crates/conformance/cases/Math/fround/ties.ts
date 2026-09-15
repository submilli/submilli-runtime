// test262: test/built-ins/Math/fround/ties.js

function main(): void {
  const a0: number = 1.0;
  const a1: number = 1.0000000596046448;
  const a2: number = 1.0000001192092896;
  const a3: number = 1.0000001788139343;
  const a4: number = 1.000000238418579;
  const a5: number = 1.0000002980232239;
  const a6: number = 1.0000003576278687;

  assertSameValue(Math.fround(a0), a0, "Math.fround(a0)");
  assertSameValue(Math.fround(a1), a0, "Math.fround(a1)");
  assertSameValue(Math.fround(a2), a2, "Math.fround(a2)");
  assertSameValue(Math.fround(a3), a4, "Math.fround(a3)");
  assertSameValue(Math.fround(a4), a4, "Math.fround(a4)");
  assertSameValue(Math.fround(a5), a4, "Math.fround(a5)");
  assertSameValue(Math.fround(a6), a6, "Math.fround(a6)");

  assertSameValue(Math.fround(-a0), -a0, "Math.fround(-a0)");
  assertSameValue(Math.fround(-a1), -a0, "Math.fround(-a1)");
  assertSameValue(Math.fround(-a2), -a2, "Math.fround(-a2)");
  assertSameValue(Math.fround(-a3), -a4, "Math.fround(-a3)");
  assertSameValue(Math.fround(-a4), -a4, "Math.fround(-a4)");
  assertSameValue(Math.fround(-a5), -a4, "Math.fround(-a5)");
  assertSameValue(Math.fround(-a6), -a6, "Math.fround(-a6)");
}
