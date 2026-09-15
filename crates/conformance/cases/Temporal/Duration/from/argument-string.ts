// test262: test/built-ins/Temporal/Duration/from/argument-string.js
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
  checkDuration(Temporal.Duration.from("P1D"),
    0, 0, 0, 1, 0, 0, 0, 0, 0, 0, "P1D");
  checkDuration(Temporal.Duration.from("p1y1m1dt1h1m1s"),
    1, 1, 0, 1, 1, 1, 1, 0, 0, 0, "lowercase");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.1S"),
    1, 1, 1, 1, 1, 1, 1, 100, 0, 0, "1 fractional digit");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.12S"),
    1, 1, 1, 1, 1, 1, 1, 120, 0, 0, "2 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.123S"),
    1, 1, 1, 1, 1, 1, 1, 123, 0, 0, "3 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.1234S"),
    1, 1, 1, 1, 1, 1, 1, 123, 400, 0, "4 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.12345S"),
    1, 1, 1, 1, 1, 1, 1, 123, 450, 0, "5 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.123456S"),
    1, 1, 1, 1, 1, 1, 1, 123, 456, 0, "6 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.1234567S"),
    1, 1, 1, 1, 1, 1, 1, 123, 456, 700, "7 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.12345678S"),
    1, 1, 1, 1, 1, 1, 1, 123, 456, 780, "8 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1.123456789S"),
    1, 1, 1, 1, 1, 1, 1, 123, 456, 789, "9 fractional digits");
  checkDuration(Temporal.Duration.from("P1Y1M1W1DT1H1M1,12S"),
    1, 1, 1, 1, 1, 1, 1, 120, 0, 0, "comma decimal separator");
  checkDuration(Temporal.Duration.from("P1DT0.5M"),
    0, 0, 0, 1, 0, 0, 30, 0, 0, 0, "fractional minutes");
  checkDuration(Temporal.Duration.from("P1DT0,5H"),
    0, 0, 0, 1, 0, 30, 0, 0, 0, 0, "fractional hours");
  checkDuration(Temporal.Duration.from("+P1D"),
    0, 0, 0, 1, 0, 0, 0, 0, 0, 0, "explicit plus sign");
  checkDuration(Temporal.Duration.from("-P1D"),
    0, 0, 0, -1, 0, 0, 0, 0, 0, 0, "minus sign");
  checkDuration(Temporal.Duration.from("-P1Y1M1W1DT1H1M1.123456789S"),
    -1, -1, -1, -1, -1, -1, -1, -123, -456, -789, "negative with all units");
  checkDuration(Temporal.Duration.from("PT100M"),
    0, 0, 0, 0, 0, 100, 0, 0, 0, 0, "unbalanced minutes");
}
