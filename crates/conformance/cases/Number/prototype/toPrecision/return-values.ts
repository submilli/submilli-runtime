// test262: test/built-ins/Number/prototype/toPrecision/return-values.js

function main(): void {
  assertSameValue((7).toPrecision(1), "7");
  assertSameValue((7).toPrecision(2), "7.0");
  assertSameValue((7).toPrecision(3), "7.00");
  assertSameValue((7).toPrecision(19), "7.000000000000000000");
  assertSameValue((7).toPrecision(20), "7.0000000000000000000");
  assertSameValue((7).toPrecision(21), "7.00000000000000000000");

  assertSameValue((-7).toPrecision(1), "-7");
  assertSameValue((-7).toPrecision(2), "-7.0");
  assertSameValue((-7).toPrecision(3), "-7.00");
  assertSameValue((-7).toPrecision(19), "-7.000000000000000000");
  assertSameValue((-7).toPrecision(20), "-7.0000000000000000000");
  assertSameValue((-7).toPrecision(21), "-7.00000000000000000000");

  assertSameValue((10).toPrecision(2), "10");
  assertSameValue((11).toPrecision(2), "11");
  assertSameValue((17).toPrecision(2), "17");
  assertSameValue((19).toPrecision(2), "19");
  assertSameValue((20).toPrecision(2), "20");

  assertSameValue((-10).toPrecision(2), "-10");
  assertSameValue((-11).toPrecision(2), "-11");
  assertSameValue((-17).toPrecision(2), "-17");
  assertSameValue((-19).toPrecision(2), "-19");
  assertSameValue((-20).toPrecision(2), "-20");

  assertSameValue((42).toPrecision(2), "42");
  assertSameValue((-42).toPrecision(2), "-42");

  assertSameValue((100).toPrecision(3), "100");
  assertSameValue((100).toPrecision(7), "100.0000");
  assertSameValue((1000).toPrecision(7), "1000.000");
  assertSameValue((10000).toPrecision(7), "10000.00");
  assertSameValue((100000).toPrecision(7), "100000.0");

  assertSameValue((0.000001).toPrecision(1), "0.000001");
  assertSameValue((0.000001).toPrecision(2), "0.0000010");
  assertSameValue((0.000001).toPrecision(3), "0.00000100");
}
