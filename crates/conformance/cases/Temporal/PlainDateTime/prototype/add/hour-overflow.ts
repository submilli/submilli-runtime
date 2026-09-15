// test262: test/built-ins/Temporal/PlainDateTime/prototype/add/hour-overflow.js
// expect-fail: PlainDateTime.nanosecond returns the full nanoseconds-within-second (271986102) instead of the standard nanosecond-within-microsecond (102); millisecond/microsecond are already standard, and ZonedDateTime.nanosecond is too
// new Temporal.PlainDateTime(...) positional args -> from(ISO string).

function checkPlainDateTime(
  dt: Temporal.PlainDateTime,
  year: number, month: number, monthCode: string, day: number,
  hour: number, minute: number, second: number,
  millisecond: number, microsecond: number, nanosecond: number,
  message: string,
): void {
  assertSameValue(dt.year, year, `${message}: year`);
  assertSameValue(dt.month, month, `${message}: month`);
  assertSameValue(dt.monthCode, monthCode, `${message}: monthCode`);
  assertSameValue(dt.day, day, `${message}: day`);
  assertSameValue(dt.hour, hour, `${message}: hour`);
  assertSameValue(dt.minute, minute, `${message}: minute`);
  assertSameValue(dt.second, second, `${message}: second`);
  assertSameValue(dt.millisecond, millisecond, `${message}: millisecond`);
  assertSameValue(dt.microsecond, microsecond, `${message}: microsecond`);
  assertSameValue(dt.nanosecond, nanosecond, `${message}: nanosecond`);
}

function main(): void {
  const earlier = Temporal.PlainDateTime.from("2020-05-31T23:12:38.271986102");

  checkPlainDateTime(
    earlier.add({ hours: 2 }),
    2020, 6, "M06", 1, 1, 12, 38, 271, 986, 102,
    "hours overflow (push to next day)",
  );

  const later = Temporal.PlainDateTime.from("2019-10-29T10:46:38.271986102");

  checkPlainDateTime(
    later.add({ hours: -12 }),
    2019, 10, "M10", 28, 22, 46, 38, 271, 986, 102,
    "hours overflow (push to previous day)",
  );
}
