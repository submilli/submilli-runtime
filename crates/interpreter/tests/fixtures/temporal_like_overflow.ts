function catchesRangeError(action: () => void): boolean {
  try { action(); return false; } catch (e) { return e instanceof RangeError; }
}
function catchesTypeError(action: () => void): boolean {
  try { action(); return false; } catch (e) { return e instanceof TypeError; }
}
function main(): void {
  assert(Temporal.PlainDate.from({ year: 2023, month: 13, day: 1 }).month === 12, "default constrain");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ year: 2023, month: 13, day: 1 }, { overflow: "reject" }); }), "reject month");
  assert(Temporal.PlainDate.from({ year: 2023, month: 2, day: 31 }, { overflow: "constrain" }).day === 28, "constrain day to month");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ year: 2023, month: 2, day: 29 }, { overflow: "reject" }); }), "reject nonleap day");
  assert(catchesTypeError(() => { Temporal.PlainDate.from({ month: 5, day: 18 }); }), "missing year");
  assert(catchesTypeError(() => { Temporal.PlainTime.from({}); }), "empty time bag");
  assert(Temporal.PlainTime.from({ hour: 27, minute: -1, second: 80 }).toString() === "23:00:59", "constrain time fields");
  assert(catchesRangeError(() => { Temporal.PlainTime.from({ hour: 24 }, { overflow: "reject" }); }), "reject hour");
  assert(Temporal.PlainDateTime.from({ year: 2023, month: 13, day: 32, hour: 24 }).toString() === "2023-12-31T23:00:00", "constrain datetime");
  assert(Temporal.PlainYearMonth.from({ year: 2023, month: 13 }).month === 12, "constrain year-month");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ year: 2023, month: 0, day: 1 }); }), "month must be positive");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ year: 2023, month: 1, day: 0 }); }), "day must be positive");
  assert(catchesRangeError(() => { Temporal.PlainYearMonth.from({ year: 2023, month: 13 }, { overflow: "reject" }); }), "reject year-month");
  assert(Temporal.PlainMonthDay.from({ year: 2023, month: 2, day: 29 }).day === 28, "month-day uses supplied year");
  assert(catchesRangeError(() => { Temporal.PlainMonthDay.from({ year: 2023, month: 2, day: 29 }, { overflow: "reject" }); }), "reject month-day");
  assert(Temporal.ZonedDateTime.from({ year: 2023, month: 13, day: 1, timeZone: "UTC" }).month === 12, "constrain zoned date");
  assert(catchesRangeError(() => { Temporal.ZonedDateTime.from({ year: 2023, month: 13, day: 1, timeZone: "UTC" }, { overflow: "reject" }); }), "reject zoned date");
  assert(Temporal.PlainDate.from({ year: 2023.9, month: 5.9, day: 18.9 }).day === 18, "truncate finite date fields");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ year: Infinity, month: 5, day: 18 }); }), "reject nonfinite year");
  assert(catchesRangeError(() => { Temporal.PlainDate.from("2023-13-01"); }), "strings never constrain");
  assert(Temporal.PlainDate.from({ year: 2023, monthCode: "M05", day: 18 }).month === 5, "ISO month code");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ year: 2023, month: 5, monthCode: "M06", day: 18 }); }), "month code must agree");
  assert(Temporal.PlainMonthDay.from({ year: 2023, monthCode: "M02", day: 29 }, { overflow: "reject" }).day === 29, "month code selects reference leap year");
  assert(Temporal.PlainMonthDay.from({ year: 2023, month: 2, monthCode: "M02", day: 29 }).day === 29, "month code takes precedence over year");
  assert(catchesRangeError(() => { Temporal.PlainMonthDay.from({ year: Infinity, monthCode: "M02", day: 29 }); }), "ignored year still converted");
  assert(Temporal.PlainDate.from({ year: 2023, month: 5, day: 18, calendar: "ISO8601" }).month === 5, "ISO calendar is case insensitive");
  assert(catchesRangeError(() => { Temporal.PlainDate.from({ month: Infinity, day: 1 }); }), "prepare fields before checking missing year");
  assert(catchesRangeError(() => { Temporal.PlainDateTime.from({ month: Infinity, day: 1 }); }), "datetime prepares fields before missing year");
  const missingDay = (((): unknown => ({ year: 2023, month: 5 })) as () => { year: number; month: number; day: number });
  assert(catchesTypeError(() => { Temporal.PlainDate.from(missingDay()); }), "missing day at erased boundary");
  const invalidOptions = (((): unknown => ({ overflow: "invalid" })) as () => { overflow: "reject" });
  assert(catchesTypeError(() => { Temporal.PlainDate.from({ month: 1, day: 1 }, invalidOptions()); }), "date fields required before overflow");
  assert(catchesRangeError(() => { Temporal.PlainYearMonth.from({}, invalidOptions()); }), "year-month validates overflow before required fields");
  assert(catchesRangeError(() => { Temporal.PlainTime.from({}, invalidOptions()); }), "time validates overflow before empty bag");
  let trace = "";
  const input = (): { year: number; month: number; day: number } => { trace += "input"; return { year: 2023, month: 5, day: 18 }; };
  const options = (): { overflow: "reject" } => { trace += "options"; return { overflow: "reject" }; };
  Temporal.PlainDate.from(input(), options());
  assert(trace === "inputoptions", "argument evaluation order and side effects");
  const erased = (value: unknown): (() => { overflow: "reject" }) => ((() => value) as () => { overflow: "reject" });
  for (const value of [null, true, 1, "reject", 1n]) {
    assert(catchesTypeError(() => { Temporal.PlainDate.from("2023-05-18", erased(value)()); }), "primitive options must throw TypeError");
    assert(catchesTypeError(() => { Temporal.PlainTime.from({ hour: 12 }, erased(value)()); }), "time primitive options must throw TypeError");
    assert(catchesTypeError(() => { Temporal.PlainYearMonth.from({ year: 2023, month: 5 }, erased(value)()); }), "year-month primitive options must throw TypeError");
    assert(catchesTypeError(() => { Temporal.ZonedDateTime.from({ year: 2023, month: 5, day: 18, timeZone: "UTC" }, erased(value)()); }), "zoned primitive options must throw TypeError");
  }
  assert(catchesRangeError(() => { Temporal.PlainDate.from("2023-05-18", erased({ overflow: 17 })()); }), "malformed overflow field is RangeError");
  assert(Temporal.PlainDate.from("2023-05-18", erased(undefined)()).day === 18, "erased undefined options defaults");
  assert(Temporal.PlainDate.from("2023-05-18", erased({})()).day === 18, "empty options object defaults");
  assert(Temporal.PlainDate.from("2023-05-18", erased(Temporal.PlainDate.from("2023-01-01"))()).day === 18, "native object options defaults");
}
