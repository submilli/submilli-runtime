// Copyright (C) 2021 the V8 project authors. All rights reserved.
// This code is governed by the BSD license found in the LICENSE file.
// test262: test/built-ins/Temporal/PlainMonthDay/from/basic.js
// Adaptation: typed option helper and field assertions replace TemporalHelpers.

function check(overflow: "constrain" | "reject"): void {
  const options = { overflow };
  assertSameValue(Temporal.PlainMonthDay.from({ year: 2021, month: 7, day: 3 }, options).toString(), "07-03");
  assertSameValue(Temporal.PlainMonthDay.from({ year: 2021, month: 12, day: 31 }, options).toString(), "12-31");
  assertSameValue(Temporal.PlainMonthDay.from({ monthCode: "M07", day: 3 }, options).toString(), "07-03");
  assertSameValue(Temporal.PlainMonthDay.from({ monthCode: "M12", day: 31 }, options).toString(), "12-31");
}
function main(): void {
  check("constrain");
  check("reject");
  assertSameValue(Temporal.PlainMonthDay.from({ year: 2021, month: 7, day: 3 }, {}).toString(), "07-03");
  assertSameValue(Temporal.PlainMonthDay.from({ year: 2021, month: 7, day: 3 }, undefined).toString(), "07-03");
  const days = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  const codes = ["M01", "M02", "M03", "M04", "M05", "M06", "M07", "M08", "M09", "M10", "M11", "M12"];
  for (let i = 0; i < 12; i++) {
    const numeric = Temporal.PlainMonthDay.from({ month: i + 1, day: days[i] });
    assertSameValue(numeric.monthCode, codes[i]);
    assertSameValue(numeric.day, days[i]);
    const coded = Temporal.PlainMonthDay.from({ monthCode: codes[i], day: days[i] });
    assertSameValue(coded.monthCode, codes[i]);
    assertSameValue(coded.day, days[i]);
  }
}
