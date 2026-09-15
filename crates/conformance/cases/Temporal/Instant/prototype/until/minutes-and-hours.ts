// test262: test/built-ins/Temporal/Instant/prototype/until/minutes-and-hours.js

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
  const feb20 = Temporal.Instant.from("2020-02-01T00:00Z");
  const feb21 = Temporal.Instant.from("2021-02-01T00:00Z");

  checkDuration(feb20.until(feb21, { largestUnit: "hours" }),
    0, 0, 0, 0, 8784, 0, 0, 0, 0, 0, "hours");
  checkDuration(feb20.until(feb21, { largestUnit: "minutes" }),
    0, 0, 0, 0, 0, 527040, 0, 0, 0, 0, "minutes");
}
