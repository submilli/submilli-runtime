// test262: test/built-ins/Temporal/PlainTime/prototype/round/rounding-cross-midnight.js
// expect-fail: Temporal.PlainTime has no round() method (rounding on the Temporal types is a known gap)

function main(): void {
  const plainTime = Temporal.PlainTime.from("23:59:59.999999999");
  const units: string[] = ["hour", "minute", "second", "millisecond", "microsecond"];
  for (const smallestUnit of units) {
    const result = plainTime.round({ smallestUnit });
    assertSameValue(result.hour, 0, `${smallestUnit}: hour`);
    assertSameValue(result.minute, 0, `${smallestUnit}: minute`);
    assertSameValue(result.second, 0, `${smallestUnit}: second`);
    assertSameValue(result.millisecond, 0, `${smallestUnit}: millisecond`);
    assertSameValue(result.microsecond, 0, `${smallestUnit}: microsecond`);
    assertSameValue(result.nanosecond, 0, `${smallestUnit}: nanosecond`);
  }
}
