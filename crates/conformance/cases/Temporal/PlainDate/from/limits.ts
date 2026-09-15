// test262: test/built-ins/Temporal/PlainDate/from/limits.js
// expect-fail: six-digit extended-year ISO strings ("-271821-04-19", "+275760-09-13") are not parsed, so the standard PlainDate range limits cannot be expressed
// Property-bag from({ year, month, day }) and the overflow option are dropped:
// from() is string-only here; the range enforcement is the point.

function main(): void {
  assertThrows((): void => {
    Temporal.PlainDate.from("-271821-04-18");
  }, "before min");
  assertThrows((): void => {
    Temporal.PlainDate.from("+275760-09-14");
  }, "after max");

  const min = Temporal.PlainDate.from("-271821-04-19");
  assertSameValue(min.year, -271821, "min string: year");
  assertSameValue(min.month, 4, "min string: month");
  assertSameValue(min.day, 19, "min string: day");

  const max = Temporal.PlainDate.from("+275760-09-13");
  assertSameValue(max.year, 275760, "max string: year");
  assertSameValue(max.month, 9, "max string: month");
  assertSameValue(max.day, 13, "max string: day");
}
