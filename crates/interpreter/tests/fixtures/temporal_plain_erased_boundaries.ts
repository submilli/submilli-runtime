function catchesTypeError(action: () => void): boolean {
  try {
    action();
    return false;
  } catch (e) {
    return e instanceof TypeError;
  }
}

function main(): void {
  const makeDate = (): Temporal.PlainDate => Temporal.PlainDate.from("2024-01-02");
  const makeTime = (): Temporal.PlainTime => Temporal.PlainTime.from("03:04:05");
  const makeDateTime = (): Temporal.PlainDateTime =>
    Temporal.PlainDateTime.from("2024-01-02T03:04:05");
  const makeYearMonth = (): Temporal.PlainYearMonth => Temporal.PlainYearMonth.from("2024-01");
  const makeMonthDay = (): Temporal.PlainMonthDay => Temporal.PlainMonthDay.from("01-02");

  assert(makeDate().day === 2, "PlainDate erased return");
  assert(makeTime().hour === 3, "PlainTime erased return");
  assert(makeDateTime().second === 5, "PlainDateTime erased return");
  assert(makeYearMonth().month === 1, "PlainYearMonth erased return");
  assert(makeMonthDay().day === 2, "PlainMonthDay erased return");

  const acceptsNullable = (value: Temporal.PlainDate | null): number =>
    value === null ? 0 : value.day;
  assert(acceptsNullable(makeDate()) === 2, "nullable plain interface parameter");
  assert(acceptsNullable(null) === 0, "nullable plain interface null parameter");

  const wrong = ((): unknown => ({ wrong: 1 })) as () => Temporal.PlainDate;
  assert(catchesTypeError(() => { wrong(); }), "wrong erased plain return");

  const monthDayAsYearMonth = ((): unknown => Temporal.PlainMonthDay.from("01-02")) as () => Temporal.PlainYearMonth;
  assert(catchesTypeError(() => { monthDayAsYearMonth(); }), "MonthDay is not YearMonth");
  const yearMonthAsMonthDay = ((): unknown => Temporal.PlainYearMonth.from("2024-01")) as () => Temporal.PlainMonthDay;
  assert(catchesTypeError(() => { yearMonthAsMonthDay(); }), "YearMonth is not MonthDay");
}
