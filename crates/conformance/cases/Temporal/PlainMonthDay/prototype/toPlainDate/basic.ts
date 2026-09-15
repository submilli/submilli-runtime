// test262: test/built-ins/Temporal/PlainMonthDay/prototype/toPlainDate/basic.js
// The toPlainDate({ something: "nothing" }) TypeError assertion and the
// overflow-getter trap are dropped: a literal without the declared `year`
// field is a compile error here, and there is no observable options getter.

function checkPlainDate(d: Temporal.PlainDate, year: number, month: number, monthCode: string, day: number, message: string): void {
  assertSameValue(d.year, year, `${message}: year`);
  assertSameValue(d.month, month, `${message}: month`);
  assertSameValue(d.monthCode, monthCode, `${message}: monthCode`);
  assertSameValue(d.day, day, `${message}: day`);
}

function main(): void {
  const md = Temporal.PlainMonthDay.from("01-22");
  const d = md.toPlainDate({ year: 2002 });
  checkPlainDate(d, 2002, 1, "M01", 22, "toPlainDate");

  const leapDay = Temporal.PlainMonthDay.from("02-29");
  checkPlainDate(leapDay.toPlainDate({ year: 2020 }), 2020, 2, "M02", 29, "leap day");
}
