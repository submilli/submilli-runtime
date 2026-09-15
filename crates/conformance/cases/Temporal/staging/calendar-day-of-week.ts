// test262: test/staging/Temporal/v8/calendar-day-of-week.js
// new Temporal.PlainDate(y, m, d) / new Temporal.PlainDateTime(...) positional
// constructors -> from(ISO string) (no class constructors).

function main(): void {
  assertSameValue(Temporal.PlainDate.from("1970-01-01").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDate.from("2000-01-01").dayOfWeek, 6);
  assertSameValue(Temporal.PlainDate.from("2021-01-15").dayOfWeek, 5);
  assertSameValue(Temporal.PlainDate.from("2020-02-15").dayOfWeek, 6);
  assertSameValue(Temporal.PlainDate.from("2000-02-15").dayOfWeek, 2);
  assertSameValue(Temporal.PlainDate.from("2021-02-15").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDate.from("2021-03-15").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDate.from("2021-04-15").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDate.from("2021-05-15").dayOfWeek, 6);
  assertSameValue(Temporal.PlainDate.from("2021-06-15").dayOfWeek, 2);
  assertSameValue(Temporal.PlainDate.from("2021-07-15").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDate.from("2021-08-15").dayOfWeek, 7);
  assertSameValue(Temporal.PlainDate.from("2021-09-15").dayOfWeek, 3);
  assertSameValue(Temporal.PlainDate.from("2021-10-15").dayOfWeek, 5);
  assertSameValue(Temporal.PlainDate.from("2021-11-15").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDate.from("2021-12-15").dayOfWeek, 3);
  assertSameValue(Temporal.PlainDateTime.from("1997-01-23T05:30:13").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDateTime.from("1996-02-23T05:30:13").dayOfWeek, 5);
  assertSameValue(Temporal.PlainDateTime.from("2000-02-23T05:30:13").dayOfWeek, 3);
  assertSameValue(Temporal.PlainDateTime.from("1997-02-23T05:30:13").dayOfWeek, 7);
  assertSameValue(Temporal.PlainDateTime.from("1997-03-23T05:30:13").dayOfWeek, 7);
  assertSameValue(Temporal.PlainDateTime.from("1997-04-23T05:30:13").dayOfWeek, 3);
  assertSameValue(Temporal.PlainDateTime.from("1997-05-23T05:30:13").dayOfWeek, 5);
  assertSameValue(Temporal.PlainDateTime.from("1997-06-23T05:30:13").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDateTime.from("1997-07-23T05:30:13").dayOfWeek, 3);
  assertSameValue(Temporal.PlainDateTime.from("1997-08-23T05:30:13").dayOfWeek, 6);
  assertSameValue(Temporal.PlainDateTime.from("1997-09-23T05:30:13").dayOfWeek, 2);
  assertSameValue(Temporal.PlainDateTime.from("1997-10-23T05:30:13").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDateTime.from("1997-11-23T05:30:13").dayOfWeek, 7);
  assertSameValue(Temporal.PlainDateTime.from("1997-12-23T05:30:13").dayOfWeek, 2);
  assertSameValue(Temporal.PlainDate.from("2019-01-18").dayOfWeek, 5);
  assertSameValue(Temporal.PlainDate.from("2020-02-18").dayOfWeek, 2);
  assertSameValue(Temporal.PlainDate.from("2019-02-18").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDate.from("2019-03-18").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDate.from("2019-04-18").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDate.from("2019-05-18").dayOfWeek, 6);
  assertSameValue(Temporal.PlainDate.from("2019-06-18").dayOfWeek, 2);
  assertSameValue(Temporal.PlainDate.from("2019-07-18").dayOfWeek, 4);
  assertSameValue(Temporal.PlainDate.from("2019-08-18").dayOfWeek, 7);
  assertSameValue(Temporal.PlainDate.from("2019-09-18").dayOfWeek, 3);
  assertSameValue(Temporal.PlainDate.from("2019-10-18").dayOfWeek, 5);
  assertSameValue(Temporal.PlainDate.from("2019-11-18").dayOfWeek, 1);
  assertSameValue(Temporal.PlainDate.from("2019-12-18").dayOfWeek, 3);
}
