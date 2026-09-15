// test262: test/built-ins/Temporal/PlainDateTime/prototype/until/inverse.js
// new Temporal.PlainDateTime(...) positional args -> from(ISO string);
// TemporalHelpers.assertDurationsEqual -> field-by-field comparison.

function checkDurationsEqual(actual: Temporal.Duration, expected: Temporal.Duration, message: string): void {
  assertSameValue(actual.years, expected.years, `${message}: years`);
  assertSameValue(actual.months, expected.months, `${message}: months`);
  assertSameValue(actual.weeks, expected.weeks, `${message}: weeks`);
  assertSameValue(actual.days, expected.days, `${message}: days`);
  assertSameValue(actual.hours, expected.hours, `${message}: hours`);
  assertSameValue(actual.minutes, expected.minutes, `${message}: minutes`);
  assertSameValue(actual.seconds, expected.seconds, `${message}: seconds`);
  assertSameValue(actual.milliseconds, expected.milliseconds, `${message}: milliseconds`);
  assertSameValue(actual.microseconds, expected.microseconds, `${message}: microseconds`);
  assertSameValue(actual.nanoseconds, expected.nanoseconds, `${message}: nanoseconds`);
}

function main(): void {
  const dt = Temporal.PlainDateTime.from("1976-11-18T15:23:30.123456789");
  const later = Temporal.PlainDateTime.from("2016-03-03T18:00:00");

  checkDurationsEqual(dt.until(later), later.since(dt), "until and since act as inverses");
}
