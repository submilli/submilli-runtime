// test262: test/built-ins/Temporal/PlainDateTime/prototype/until/basic.js
// Property-bag from({ year, monthCode, ... }) -> from(ISO string): the bag
// form is not part of our string-only from(); the diff math is the point.

function checkDuration(
  d: Temporal.Duration,
  years: number, months: number, weeks: number, days: number,
  hours: number, minutes: number, seconds: number,
  milliseconds: number, microseconds: number, nanoseconds: number,
  message: string,
): void {
  assertSameValue(d.years, years, `${message}: years`);
  assertSameValue(d.months, months, `${message}: months`);
  assertSameValue(d.weeks, weeks, `${message}: weeks`);
  assertSameValue(d.days, days, `${message}: days`);
  assertSameValue(d.hours, hours, `${message}: hours`);
  assertSameValue(d.minutes, minutes, `${message}: minutes`);
  assertSameValue(d.seconds, seconds, `${message}: seconds`);
  assertSameValue(d.milliseconds, milliseconds, `${message}: milliseconds`);
  assertSameValue(d.microseconds, microseconds, `${message}: microseconds`);
  assertSameValue(d.nanoseconds, nanoseconds, `${message}: nanoseconds`);
}

function main(): void {
  const date = Temporal.PlainDateTime.from("1969-07-24T12:34:00");
  const date2 = Temporal.PlainDateTime.from("1969-10-05T12:34:00");
  checkDuration(date2.until(date, { largestUnit: "days" }),
    0, 0, 0, -73, 0, 0, 0, 0, 0, 0, "same year");

  const earlier = date;
  const later = Temporal.PlainDateTime.from("1996-03-03T12:34:00");
  checkDuration(later.until(earlier, { largestUnit: "days" }),
    0, 0, 0, -9719, 0, 0, 0, 0, 0, 0, "different year");

  // Years
  const date19971201 = Temporal.PlainDateTime.from("1997-12-01T12:34:00");
  const date20010618 = Temporal.PlainDateTime.from("2001-06-18T12:34:00");
  checkDuration(date19971201.until(date20010618, { largestUnit: "years" }),
    3, 6, 0, 17, 0, 0, 0, 0, 0, 0, "3 years, 6 months, 17 days");

  // Months
  const date20001201 = Temporal.PlainDateTime.from("2000-12-01T12:34:00");
  const date20010601 = Temporal.PlainDateTime.from("2001-06-01T12:34:00");
  checkDuration(date20001201.until(date20010601, { largestUnit: "months" }),
    0, 6, 0, 0, 0, 0, 0, 0, 0, 0, "6 months");

  // Weeks
  const date20000101 = Temporal.PlainDateTime.from("2000-01-01T12:34:00");
  const date20001007 = Temporal.PlainDateTime.from("2000-10-07T12:34:00");
  checkDuration(date20000101.until(date20001007, { largestUnit: "weeks" }),
    0, 0, 40, 0, 0, 0, 0, 0, 0, 0, "40 weeks");

  // Days
  checkDuration(date20000101.until(date20001007, { largestUnit: "days" }),
    0, 0, 0, 280, 0, 0, 0, 0, 0, 0, "40 weeks in days");
}
