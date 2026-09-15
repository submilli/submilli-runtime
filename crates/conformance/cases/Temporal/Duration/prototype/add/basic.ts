// test262: test/built-ins/Temporal/Duration/prototype/add/basic.js
// The "incorrectly-spelled properties are ignored" assertion is dropped: a
// misspelled field in a typed DurationFields literal is a compile error here.

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
  const duration1 = Temporal.Duration.from({ days: 1, minutes: 5 });
  checkDuration(duration1.add({ days: 2, minutes: 5 }),
    0, 0, 0, 3, 0, 10, 0, 0, 0, 0, "positive same units");
  checkDuration(duration1.add({ hours: 12, seconds: 30 }),
    0, 0, 0, 1, 12, 5, 30, 0, 0, 0, "positive different units");
  checkDuration(Temporal.Duration.from("P3DT10M").add({ days: -2, minutes: -5 }),
    0, 0, 0, 1, 0, 5, 0, 0, 0, 0, "negative same units");
  checkDuration(Temporal.Duration.from("P1DT12H5M30S").add({ hours: -12, seconds: -30 }),
    0, 0, 0, 1, 0, 5, 0, 0, 0, 0, "negative different units");
  const duration2 = Temporal.Duration.from("P50DT50H50M50.500500500S");
  checkDuration(duration2.add(duration2),
    0, 0, 0, 104, 5, 41, 41, 1, 1, 0, "balancing positive");
  const duration3 = Temporal.Duration.from({ hours: -1, seconds: -60 });
  checkDuration(duration3.add({ minutes: 122 }),
    0, 0, 0, 0, 1, 1, 0, 0, 0, 0, "balancing flipped sign 1");
  const duration4 = Temporal.Duration.from({ hours: -1, seconds: -3721 });
  checkDuration(duration4.add({ minutes: 61, nanoseconds: 3722000000001 }),
    0, 0, 0, 0, 0, 1, 1, 0, 0, 1, "balancing flipped sign 2");
}
