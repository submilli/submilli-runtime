// test262: test/built-ins/Temporal/PlainYearMonth/prototype/toPlainDate/basic.js
// The toPlainDate({ something: "nothing" }) TypeError assertion is dropped:
// a literal without the declared `day` field is a compile error here.

function main(): void {
  const ym = Temporal.PlainYearMonth.from("2002-01");
  const d = ym.toPlainDate({ day: 22 });
  assertSameValue(d.year, 2002, "year");
  assertSameValue(d.month, 1, "month");
  assertSameValue(d.monthCode, "M01", "monthCode");
  assertSameValue(d.day, 22, "day");
}
