// test262: test/built-ins/Temporal/PlainDate/prototype/inLeapYear/basic.js
// new Temporal.PlainDate(y, m, d) -> from(ISO string) (no class constructors).

function main(): void {
  assertSameValue(Temporal.PlainDate.from("1976-11-18").inLeapYear, true, "leap year");
  assertSameValue(Temporal.PlainDate.from("1977-11-18").inLeapYear, false, "non-leap year");
  assertSameValue(Temporal.PlainDate.from("1995-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("1996-07-15").inLeapYear, true);
  assertSameValue(Temporal.PlainDate.from("1997-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("1998-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("1999-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2000-07-15").inLeapYear, true);
  assertSameValue(Temporal.PlainDate.from("2001-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2002-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2003-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2004-07-15").inLeapYear, true);
  assertSameValue(Temporal.PlainDate.from("2005-07-15").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2019-03-18").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2020-03-18").inLeapYear, true);
  assertSameValue(Temporal.PlainDate.from("2021-03-18").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2022-03-18").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2023-03-18").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2024-03-18").inLeapYear, true);
  assertSameValue(Temporal.PlainDate.from("2025-03-18").inLeapYear, false);
  assertSameValue(Temporal.PlainDate.from("2026-03-18").inLeapYear, false);
}
