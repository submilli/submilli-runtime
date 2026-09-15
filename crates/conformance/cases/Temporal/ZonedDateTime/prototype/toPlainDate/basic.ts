// test262: test/built-ins/Temporal/ZonedDateTime/prototype/toPlainDate/basic.js

function main(): void {
  const zdt = Temporal.ZonedDateTime.from("2019-10-29T09:46:38.271986102[-07:00]");

  const d = zdt.toPlainDate();
  assertSameValue(d.year, 2019, "year");
  assertSameValue(d.month, 10, "month");
  assertSameValue(d.monthCode, "M10", "monthCode");
  assertSameValue(d.day, 29, "day");
}
