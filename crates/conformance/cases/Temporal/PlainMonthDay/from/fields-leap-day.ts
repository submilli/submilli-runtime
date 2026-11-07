// Copyright (C) 2021 Igalia, S.L. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.
// test262: test/built-ins/Temporal/PlainMonthDay/from/fields-leap-day.js
// Adaptation: typed overflow helper, field assertions, and explicit RangeError checks.

function check(overflow: "constrain" | "reject"): void {
  assertSameValue(Temporal.PlainMonthDay.from("02-29", { overflow }).day, 29);
  assertSameValue(Temporal.PlainMonthDay.from({ monthCode: "M02", day: 29 }, { overflow }).day, 29);
  assertSameValue(Temporal.PlainMonthDay.from({ month: 2, day: 29 }, { overflow }).day, 29);
  assertSameValue(Temporal.PlainMonthDay.from({ month: 2, day: 29, year: 1996 }, { overflow }).day, 29);
}
function main(): void {
  check("reject");
  check("constrain");
  let caught = false;
  try { Temporal.PlainMonthDay.from({ month: 2, day: 29, year: 2001 }, { overflow: "reject" }); }
  catch (e) { caught = e instanceof RangeError; }
  assertSameValue(caught, true);
  assertSameValue(Temporal.PlainMonthDay.from({ month: 2, day: 29, year: 2001 }, { overflow: "constrain" }).day, 28);
  caught = false;
  try { Temporal.PlainMonthDay.from({ month: 2, day: 29, year: 2001, calendar: "iso8601" }, { overflow: "reject" }); }
  catch (e) { caught = e instanceof RangeError; }
  assertSameValue(caught, true);
  assertSameValue(Temporal.PlainMonthDay.from({ month: 2, day: 29, year: 2001, calendar: "iso8601" }, { overflow: "constrain" }).day, 28);
}
