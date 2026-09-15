// test262: test/built-ins/Temporal/PlainDate/prototype/add/basic.js
// new Temporal.Duration(y, m, w, d) positional args -> Duration.from bags;
// the [dateString, duration, resultString] tuple table -> checkAdd helper.

function checkPlainDate(d: Temporal.PlainDate, year: number, month: number, monthCode: string, day: number, message: string): void {
  assertSameValue(d.year, year, `${message}: year`);
  assertSameValue(d.month, month, `${message}: month`);
  assertSameValue(d.monthCode, monthCode, `${message}: monthCode`);
  assertSameValue(d.day, day, `${message}: day`);
}

function checkAdd(dateString: string, duration: Temporal.Duration, resultString: string): void {
  const date = Temporal.PlainDate.from(dateString);
  const result = Temporal.PlainDate.from(resultString);
  assert(date.add(duration).equals(result), `${dateString} + ${duration.toString()} = ${resultString}`);
}

function main(): void {
  const date = Temporal.PlainDate.from("1976-11-18");
  checkPlainDate(date.add({ years: 43 }), 2019, 11, "M11", 18, "add years");
  checkPlainDate(date.add({ months: 3 }), 1977, 2, "M02", 18, "add months");
  checkPlainDate(date.add({ days: 20 }), 1976, 12, "M12", 8, "add days");
  checkPlainDate(Temporal.PlainDate.from("2019-01-31").add({ months: 1 }), 2019, 2, "M02", 28, "constrain month end");
  checkPlainDate(date.add(Temporal.Duration.from("P43Y")), 2019, 11, "M11", 18, "add Duration");
  checkPlainDate(Temporal.PlainDate.from("2019-11-18").add({ years: -43 }), 1976, 11, "M11", 18, "subtract years");
  checkPlainDate(Temporal.PlainDate.from("1977-02-18").add({ months: -3 }), 1976, 11, "M11", 18, "subtract months");
  checkPlainDate(Temporal.PlainDate.from("1976-12-08").add({ days: -20 }), 1976, 11, "M11", 18, "subtract days");
  checkPlainDate(Temporal.PlainDate.from("2019-02-28").add({ months: -1 }), 2019, 1, "M01", 28, "subtract month end");

  const p1y = Temporal.Duration.from({ years: 1 });
  const p4y = Temporal.Duration.from({ years: 4 });
  const p5m = Temporal.Duration.from({ months: 5 });
  const p1y2m = Temporal.Duration.from({ years: 1, months: 2 });
  const p1y4d = Temporal.Duration.from({ years: 1, days: 4 });
  const p1y2m4d = Temporal.Duration.from({ years: 1, months: 2, days: 4 });
  const p10d = Temporal.Duration.from({ days: 10 });
  const p1w = Temporal.Duration.from({ weeks: 1 });
  const p6w = Temporal.Duration.from({ weeks: 6 });
  const p2w3d = Temporal.Duration.from({ weeks: 2, days: 3 });
  const p1y2w = Temporal.Duration.from({ years: 1, weeks: 2 });
  const p2m3w = Temporal.Duration.from({ months: 2, weeks: 3 });

  checkAdd("2020-02-29", p1y, "2021-02-28");
  checkAdd("2020-02-29", p4y, "2024-02-29");
  checkAdd("2021-07-16", p1y, "2022-07-16");
  checkAdd("2021-07-16", p5m, "2021-12-16");
  checkAdd("2021-08-16", p5m, "2022-01-16");
  checkAdd("2021-10-31", p5m, "2022-03-31");
  checkAdd("2021-09-30", p5m, "2022-02-28");
  checkAdd("2019-09-30", p5m, "2020-02-29");
  checkAdd("2019-10-01", p5m, "2020-03-01");
  checkAdd("2021-07-16", p1y2m, "2022-09-16");
  checkAdd("2021-11-30", p1y2m, "2023-01-30");
  checkAdd("2021-12-31", p1y2m, "2023-02-28");
  checkAdd("2022-12-31", p1y2m, "2024-02-29");
  checkAdd("2021-07-16", p1y4d, "2022-07-20");
  checkAdd("2021-02-27", p1y4d, "2022-03-03");
  checkAdd("2023-02-27", p1y4d, "2024-03-02");
  checkAdd("2021-12-30", p1y4d, "2023-01-03");
  checkAdd("2021-07-30", p1y4d, "2022-08-03");
  checkAdd("2021-06-30", p1y4d, "2022-07-04");
  checkAdd("2021-07-16", p1y2m4d, "2022-09-20");
  checkAdd("2021-02-27", p1y2m4d, "2022-05-01");
  checkAdd("2021-02-26", p1y2m4d, "2022-04-30");
  checkAdd("2023-02-26", p1y2m4d, "2024-04-30");
  checkAdd("2021-12-30", p1y2m4d, "2023-03-04");
  checkAdd("2021-07-30", p1y2m4d, "2022-10-04");
  checkAdd("2021-06-30", p1y2m4d, "2022-09-03");
  checkAdd("2021-07-16", p10d, "2021-07-26");
  checkAdd("2021-07-26", p10d, "2021-08-05");
  checkAdd("2021-12-26", p10d, "2022-01-05");
  checkAdd("2020-02-26", p10d, "2020-03-07");
  checkAdd("2021-02-26", p10d, "2021-03-08");
  checkAdd("2020-02-19", p10d, "2020-02-29");
  checkAdd("2021-02-19", p10d, "2021-03-01");
  checkAdd("2021-02-19", p1w, "2021-02-26");
  checkAdd("2021-02-27", p1w, "2021-03-06");
  checkAdd("2020-02-27", p1w, "2020-03-05");
  checkAdd("2021-12-24", p1w, "2021-12-31");
  checkAdd("2021-12-27", p1w, "2022-01-03");
  checkAdd("2021-01-27", p1w, "2021-02-03");
  checkAdd("2021-06-27", p1w, "2021-07-04");
  checkAdd("2021-07-27", p1w, "2021-08-03");
  checkAdd("2021-02-19", p6w, "2021-04-02");
  checkAdd("2021-02-27", p6w, "2021-04-10");
  checkAdd("2020-02-27", p6w, "2020-04-09");
  checkAdd("2021-12-24", p6w, "2022-02-04");
  checkAdd("2021-12-27", p6w, "2022-02-07");
  checkAdd("2021-01-27", p6w, "2021-03-10");
  checkAdd("2021-06-27", p6w, "2021-08-08");
  checkAdd("2021-07-27", p6w, "2021-09-07");
  checkAdd("2020-02-29", p2w3d, "2020-03-17");
  checkAdd("2020-02-28", p2w3d, "2020-03-16");
  checkAdd("2021-02-28", p2w3d, "2021-03-17");
  checkAdd("2020-12-28", p2w3d, "2021-01-14");
  checkAdd("2020-02-29", p1y2w, "2021-03-14");
  checkAdd("2020-02-28", p1y2w, "2021-03-14");
  checkAdd("2021-02-28", p1y2w, "2022-03-14");
  checkAdd("2020-12-28", p1y2w, "2022-01-11");
  checkAdd("2020-02-29", p2m3w, "2020-05-20");
  checkAdd("2020-02-28", p2m3w, "2020-05-19");
  checkAdd("2021-02-28", p2m3w, "2021-05-19");
  checkAdd("2020-12-28", p2m3w, "2021-03-21");
  checkAdd("2019-12-28", p2m3w, "2020-03-20");
  checkAdd("2019-10-28", p2m3w, "2020-01-18");
  checkAdd("2019-10-31", p2m3w, "2020-01-21");
}
