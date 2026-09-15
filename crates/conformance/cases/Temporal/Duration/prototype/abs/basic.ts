// test262: test/built-ins/Temporal/Duration/prototype/abs/basic.js
// expect-fail: Duration slots are capped well below the standard's 2^32 limit — the 1e5-years rows throw "overflows the maximum duration"
// new Temporal.Duration(...) positional args -> field-bag form.

function checkDuration(
  d: Temporal.Duration,
  years: number, months: number, weeks: number, days: number,
  hours: number, minutes: number, seconds: number,
  milliseconds: number, microseconds: number, nanoseconds: number,
  message: string,
): void {
  assertSameValue(d.years, years, `${message}: years`);
  assertSameValue(d.months, months, `${message}: months`);
  assertSameValue(d.weeks, weeks, `${message}: weeks`);
  assertSameValue(d.days, days, `${message}: days`);
  assertSameValue(d.hours, hours, `${message}: hours`);
  assertSameValue(d.minutes, minutes, `${message}: minutes`);
  assertSameValue(d.seconds, seconds, `${message}: seconds`);
  assertSameValue(d.milliseconds, milliseconds, `${message}: milliseconds`);
  assertSameValue(d.microseconds, microseconds, `${message}: microseconds`);
  assertSameValue(d.nanoseconds, nanoseconds, `${message}: nanoseconds`);
}

function main(): void {
  const d1 = new Temporal.Duration({});
  checkDuration(d1.abs(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, "blank");

  const d2 = new Temporal.Duration({ years: 1, months: 2, weeks: 3, days: 4, hours: 5, minutes: 6, seconds: 7, milliseconds: 8, microseconds: 9, nanoseconds: 10 });
  checkDuration(d2.abs(), 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, "positive");

  const d3 = new Temporal.Duration({ years: 1e5, months: 2e5, weeks: 3e5, days: 4e5, hours: 5e5, minutes: 6e5, seconds: 7e5, milliseconds: 8e5, microseconds: 9e5, nanoseconds: 10e5 });
  checkDuration(d3.abs(), 1e5, 2e5, 3e5, 4e5, 5e5, 6e5, 7e5, 8e5, 9e5, 10e5, "large positive");

  const d4 = new Temporal.Duration({ years: -1, months: -2, weeks: -3, days: -4, hours: -5, minutes: -6, seconds: -7, milliseconds: -8, microseconds: -9, nanoseconds: -10 });
  checkDuration(d4.abs(), 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, "negative");

  // Test with some zeros
  const d5 = new Temporal.Duration({ years: 1, weeks: 3, hours: 5, seconds: 7, microseconds: 9 });
  checkDuration(d5.abs(), 1, 0, 3, 0, 5, 0, 7, 0, 9, 0, "some zeros");

  const d6 = new Temporal.Duration({ months: 2, days: 4, minutes: 6, milliseconds: 8, nanoseconds: 10 });
  checkDuration(d6.abs(), 0, 2, 0, 4, 0, 6, 0, 8, 0, 10, "other zeros");
}
