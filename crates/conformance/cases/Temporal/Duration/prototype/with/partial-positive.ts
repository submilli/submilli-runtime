// test262: test/built-ins/Temporal/Duration/prototype/with/partial-positive.js
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
  const durationlike1 = { years: 9, hours: 5 };
  const durationlike2 = { months: 8, minutes: 4 };
  const durationlike3 = { weeks: 7, seconds: 3 };
  const durationlike4 = { days: 6, milliseconds: 2 };
  const durationlike5 = { microseconds: 987, nanoseconds: 123 };

  const d1 = new Temporal.Duration({});
  checkDuration(d1.with(durationlike1),
    9, 0, 0, 0, 5, 0, 0, 0, 0, 0, "replace all zeroes with years and hours");
  checkDuration(d1.with(durationlike2),
    0, 8, 0, 0, 0, 4, 0, 0, 0, 0, "replace all zeroes with months and minutes");
  checkDuration(d1.with(durationlike3),
    0, 0, 7, 0, 0, 0, 3, 0, 0, 0, "replace all zeroes with weeks and seconds");
  checkDuration(d1.with(durationlike4),
    0, 0, 0, 6, 0, 0, 0, 2, 0, 0, "replace all zeroes with days and milliseconds");
  checkDuration(d1.with(durationlike5),
    0, 0, 0, 0, 0, 0, 0, 0, 987, 123, "replace all zeroes with microseconds and nanoseconds");

  const d2 = new Temporal.Duration({ years: 1, months: 2, weeks: 3, days: 4, hours: 5, minutes: 6, seconds: 7, milliseconds: 8, microseconds: 9, nanoseconds: 10 });
  checkDuration(d2.with(durationlike1),
    9, 2, 3, 4, 5, 6, 7, 8, 9, 10, "replace all positive with years and hours");
  checkDuration(d2.with(durationlike2),
    1, 8, 3, 4, 5, 4, 7, 8, 9, 10, "replace all positive with months and minutes");
  checkDuration(d2.with(durationlike3),
    1, 2, 7, 4, 5, 6, 3, 8, 9, 10, "replace all positive with weeks and seconds");
  checkDuration(d2.with(durationlike4),
    1, 2, 3, 6, 5, 6, 7, 2, 9, 10, "replace all positive with days and milliseconds");
  checkDuration(d2.with(durationlike5),
    1, 2, 3, 4, 5, 6, 7, 8, 987, 123, "replace all positive with microseconds and nanoseconds");
}
