// test262: test/built-ins/Temporal/Duration/prototype/round/days-24-hours.js
// new Temporal.Duration(...) positional args -> field-bag form.

function main(): void {
  const hours25 = new Temporal.Duration({ hours: 25 });

  const rounded = hours25.round({ largestUnit: "days" });
  assertSameValue(rounded.years, 0, "years");
  assertSameValue(rounded.months, 0, "months");
  assertSameValue(rounded.weeks, 0, "weeks");
  assertSameValue(rounded.days, 1, "days");
  assertSameValue(rounded.hours, 1, "hours");
  assertSameValue(rounded.minutes, 0, "minutes");
  assertSameValue(rounded.seconds, 0, "seconds");
  assertSameValue(rounded.milliseconds, 0, "milliseconds");
  assertSameValue(rounded.microseconds, 0, "microseconds");
  assertSameValue(rounded.nanoseconds, 0, "nanoseconds");
}
