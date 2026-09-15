// test262: test/built-ins/Temporal/PlainDate/from/argument-string.js
// expect-fail: PlainDate.from accepts only the extended four-digit-year ISO form — the basic (compact "19761118") forms, six-digit extended years ("+010583-06-30", "-010583-06-30"), and datetime strings with offsets are rejected

function checkFrom(input: string, year: number, month: number, monthCode: string, day: number): void {
  const result = Temporal.PlainDate.from(input);
  assertSameValue(result.year, year, `from(${input}): year`);
  assertSameValue(result.month, month, `from(${input}): month`);
  assertSameValue(result.monthCode, monthCode, `from(${input}): monthCode`);
  assertSameValue(result.day, day, `from(${input}): day`);
}

function main(): void {
  checkFrom("1976-11-18", 1976, 11, "M11", 18);
  checkFrom("2019-06-30", 2019, 6, "M06", 30);
  checkFrom("+000050-06-30", 50, 6, "M06", 30);
  checkFrom("+010583-06-30", 10583, 6, "M06", 30);
  checkFrom("-010583-06-30", -10583, 6, "M06", 30);
  checkFrom("-000333-06-30", -333, 6, "M06", 30);
  checkFrom("19761118", 1976, 11, "M11", 18);
  checkFrom("+0019761118", 1976, 11, "M11", 18);
  checkFrom("1976-11-18T152330.1+00:00", 1976, 11, "M11", 18);
  checkFrom("19761118T15:23:30.1+00:00", 1976, 11, "M11", 18);
  checkFrom("1976-11-18T15:23:30.1+0000", 1976, 11, "M11", 18);
  checkFrom("1976-11-18T152330.1+0000", 1976, 11, "M11", 18);
  checkFrom("19761118T15:23:30.1+0000", 1976, 11, "M11", 18);
  checkFrom("19761118T152330.1+00:00", 1976, 11, "M11", 18);
  checkFrom("19761118T152330.1+0000", 1976, 11, "M11", 18);
  checkFrom("+001976-11-18T152330.1+00:00", 1976, 11, "M11", 18);
  checkFrom("+0019761118T15:23:30.1+00:00", 1976, 11, "M11", 18);
  checkFrom("+001976-11-18T15:23:30.1+0000", 1976, 11, "M11", 18);
  checkFrom("+001976-11-18T152330.1+0000", 1976, 11, "M11", 18);
  checkFrom("+0019761118T15:23:30.1+0000", 1976, 11, "M11", 18);
  checkFrom("+0019761118T152330.1+00:00", 1976, 11, "M11", 18);
  checkFrom("+0019761118T152330.1+0000", 1976, 11, "M11", 18);
}
