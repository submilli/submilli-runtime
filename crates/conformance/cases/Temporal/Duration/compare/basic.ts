// test262: test/built-ins/Temporal/Duration/compare/basic.js
// new Temporal.Duration(...) positional args -> field-bag form; RangeError -> Error.

function main(): void {
  const td1pos = new Temporal.Duration({ hours: 5, minutes: 5, seconds: 5, milliseconds: 5, microseconds: 5, nanoseconds: 5 });
  const td2pos = new Temporal.Duration({ hours: 5, minutes: 4, seconds: 5, milliseconds: 5, microseconds: 5, nanoseconds: 5 });
  const td1neg = new Temporal.Duration({ hours: -5, minutes: -5, seconds: -5, milliseconds: -5, microseconds: -5, nanoseconds: -5 });
  const td2neg = new Temporal.Duration({ hours: -5, minutes: -4, seconds: -5, milliseconds: -5, microseconds: -5, nanoseconds: -5 });
  assertSameValue(Temporal.Duration.compare(td1pos, td1pos), 0,
    "time units: equal");
  assertSameValue(Temporal.Duration.compare(td2pos, td1pos), -1,
    "time units: smaller/larger");
  assertSameValue(Temporal.Duration.compare(td1pos, td2pos), 1,
    "time units: larger/smaller");
  assertSameValue(Temporal.Duration.compare(td1neg, td1neg), 0,
    "time units: negative/negative equal");
  assertSameValue(Temporal.Duration.compare(td2neg, td1neg), 1,
    "time units: negative/negative smaller/larger");
  assertSameValue(Temporal.Duration.compare(td1neg, td2neg), -1,
    "time units: negative/negative larger/smaller");
  assertSameValue(Temporal.Duration.compare(td1neg, td2pos), -1,
    "time units: negative/positive");
  assertSameValue(Temporal.Duration.compare(td1pos, td2neg), 1,
    "time units: positive/negative");

  const dd1pos = new Temporal.Duration({ years: 5, months: 5, weeks: 5, days: 5, hours: 5, minutes: 5, seconds: 5, milliseconds: 5, microseconds: 5, nanoseconds: 5 });
  const dd2pos = new Temporal.Duration({ years: 5, months: 5, weeks: 5, days: 5, hours: 5, minutes: 4, seconds: 5, milliseconds: 5, microseconds: 5, nanoseconds: 5 });
  const dd1neg = new Temporal.Duration({ years: -5, months: -5, weeks: -5, days: -5, hours: -5, minutes: -5, seconds: -5, milliseconds: -5, microseconds: -5, nanoseconds: -5 });
  const dd2neg = new Temporal.Duration({ years: -5, months: -5, weeks: -5, days: -5, hours: -5, minutes: -4, seconds: -5, milliseconds: -5, microseconds: -5, nanoseconds: -5 });
  const relativeTo = Temporal.PlainDate.from("2017-01-01");
  assertThrows((): void => {
    Temporal.Duration.compare(dd1pos, dd2pos);
  }, "date units: relativeTo is required");
  assertSameValue(Temporal.Duration.compare(dd1pos, dd1pos, { relativeTo }), 0,
    "date units: equal");
  assertSameValue(Temporal.Duration.compare(dd2pos, dd1pos, { relativeTo }), -1,
    "date units: smaller/larger");
  assertSameValue(Temporal.Duration.compare(dd1pos, dd2pos, { relativeTo }), 1,
    "date units: larger/smaller");
  assertSameValue(Temporal.Duration.compare(dd1neg, dd1neg, { relativeTo }), 0,
    "date units: negative/negative equal");
  assertSameValue(Temporal.Duration.compare(dd2neg, dd1neg, { relativeTo }), 1,
    "date units: negative/negative smaller/larger");
  assertSameValue(Temporal.Duration.compare(dd1neg, dd2neg, { relativeTo }), -1,
    "date units: negative/negative larger/smaller");
  assertSameValue(Temporal.Duration.compare(dd1neg, dd2pos, { relativeTo }), -1,
    "date units: negative/positive");
  assertSameValue(Temporal.Duration.compare(dd1pos, dd2neg, { relativeTo }), 1,
    "date units: positive/negative");
}
