// test262: test/built-ins/Temporal/PlainTime/prototype/add/argument-object.js
// expect-fail: PlainTime.nanosecond returns the full nanoseconds-within-second (123456789) instead of the standard nanosecond-within-microsecond (789); millisecond/microsecond are already standard, and ZonedDateTime.nanosecond is too
// new Temporal.PlainTime(...) positional args -> from(ISO string); the
// "misspelled property is ignored" assertion is dropped (a misspelled field
// in a typed DurationFields literal is a compile error here).

function checkPlainTime(
  t: Temporal.PlainTime,
  hour: number, minute: number, second: number,
  millisecond: number, microsecond: number, nanosecond: number,
  message: string,
): void {
  assertSameValue(t.hour, hour, `${message}: hour`);
  assertSameValue(t.minute, minute, `${message}: minute`);
  assertSameValue(t.second, second, `${message}: second`);
  assertSameValue(t.millisecond, millisecond, `${message}: millisecond`);
  assertSameValue(t.microsecond, microsecond, `${message}: microsecond`);
  assertSameValue(t.nanosecond, nanosecond, `${message}: nanosecond`);
}

function main(): void {
  const plainTime = Temporal.PlainTime.from("15:23:30.123456789");
  checkPlainTime(plainTime.add({ hours: 16 }),
    7, 23, 30, 123, 456, 789, "add 16 hours across midnight boundary");
  checkPlainTime(plainTime.add({ minutes: 45 }),
    16, 8, 30, 123, 456, 789, "add 45 minutes");
  checkPlainTime(plainTime.add({ seconds: 800 }),
    15, 36, 50, 123, 456, 789, "add 800 seconds");
  checkPlainTime(plainTime.add({ milliseconds: 800 }),
    15, 23, 30, 923, 456, 789, "add 800 milliseconds");
  checkPlainTime(plainTime.add({ microseconds: 800 }),
    15, 23, 30, 124, 256, 789, "add 800 microseconds");
  checkPlainTime(plainTime.add({ nanoseconds: 300 }),
    15, 23, 30, 123, 457, 89, "add 300 nanoseconds");
  checkPlainTime(Temporal.PlainTime.from("07:23:30.123456789").add({ hours: -16 }),
    15, 23, 30, 123, 456, 789, "add -16 hours across midnight boundary");
  checkPlainTime(Temporal.PlainTime.from("16:08:30.123456789").add({ minutes: -45 }),
    15, 23, 30, 123, 456, 789, "add -45 minutes");
  checkPlainTime(Temporal.PlainTime.from("15:36:50.123456789").add({ seconds: -800 }),
    15, 23, 30, 123, 456, 789, "add -800 seconds");
  checkPlainTime(Temporal.PlainTime.from("15:23:30.923456789").add({ milliseconds: -800 }),
    15, 23, 30, 123, 456, 789, "add -800 milliseconds");
  checkPlainTime(Temporal.PlainTime.from("15:23:30.124256789").add({ microseconds: -800 }),
    15, 23, 30, 123, 456, 789, "add -800 microseconds");
  checkPlainTime(Temporal.PlainTime.from("15:23:30.123457089").add({ nanoseconds: -300 }),
    15, 23, 30, 123, 456, 789, "add -300 nanoseconds");
}
