// test262: test/built-ins/Temporal/PlainYearMonth/prototype/add/argument-object.js
// The { overflow: "constrain" } / { overflow: "reject" } option variants are
// dropped: add() takes no overflow options here. The tuple table -> a helper.

function checkAdd(ym: Temporal.PlainYearMonth, added: Temporal.PlainYearMonth, year: number, month: number, monthCode: string): void {
  assertSameValue(added.year, year, `${ym.toString()}: year`);
  assertSameValue(added.month, month, `${ym.toString()}: month`);
  assertSameValue(added.monthCode, monthCode, `${ym.toString()}: monthCode`);
}

function main(): void {
  const ym = Temporal.PlainYearMonth.from("2019-11");

  checkAdd(ym, ym.add({ months: 2 }), 2020, 1, "M01");
  checkAdd(ym, ym.add({ years: 1 }), 2020, 11, "M11");
  checkAdd(ym, ym.add({ months: -2 }), 2019, 9, "M09");
  checkAdd(ym, ym.add({ years: -1 }), 2018, 11, "M11");
}
