// test262: test/built-ins/Temporal/PlainDate/prototype/daysInMonth/basic.js
// new Temporal.PlainDate(y, m, d) -> from(ISO string); the [date, expected]
// tuple table -> a checked helper.

function checkDaysInMonth(dateString: string, expected: number): void {
  const plainDate = Temporal.PlainDate.from(dateString);
  assertSameValue(plainDate.daysInMonth, expected, `${expected} days in the month of ${dateString}`);
}

function main(): void {
  checkDaysInMonth("1976-02-18", 29);
  checkDaysInMonth("1976-11-18", 30);
  checkDaysInMonth("1976-12-18", 31);
  checkDaysInMonth("1977-02-18", 28);
  checkDaysInMonth("2021-01-15", 31);
  checkDaysInMonth("2020-02-15", 29);
  checkDaysInMonth("2000-02-15", 29);
  checkDaysInMonth("2021-02-15", 28);
  checkDaysInMonth("2021-03-15", 31);
  checkDaysInMonth("2021-04-15", 30);
  checkDaysInMonth("2021-05-15", 31);
  checkDaysInMonth("2021-06-15", 30);
  checkDaysInMonth("2021-07-15", 31);
  checkDaysInMonth("2021-08-15", 31);
  checkDaysInMonth("2021-09-15", 30);
  checkDaysInMonth("2021-10-15", 31);
  checkDaysInMonth("2021-11-15", 30);
  checkDaysInMonth("2021-12-15", 31);
  checkDaysInMonth("2019-01-18", 31);
  checkDaysInMonth("2020-02-18", 29);
  checkDaysInMonth("2019-02-18", 28);
  checkDaysInMonth("2019-03-18", 31);
  checkDaysInMonth("2019-04-18", 30);
  checkDaysInMonth("2019-05-18", 31);
  checkDaysInMonth("2019-06-18", 30);
  checkDaysInMonth("2019-07-18", 31);
  checkDaysInMonth("2019-08-18", 31);
  checkDaysInMonth("2019-09-18", 30);
  checkDaysInMonth("2019-10-18", 31);
  checkDaysInMonth("2019-11-18", 30);
  checkDaysInMonth("2019-12-18", 31);
}
