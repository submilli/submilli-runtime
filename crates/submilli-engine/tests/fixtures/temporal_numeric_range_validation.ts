function main(): void {
  const zoned = Temporal.ZonedDateTime.from(
    "2024-03-09T15:30:45-05:00[America/New_York]",
  );

  for (const year of [65541, 12000]) {
    let rejected = false;
    try {
      zoned.with({ year });
    } catch (error: Error) {
      rejected =
        error.message.includes("Temporal.ZonedDateTime.with{year}") &&
        error.message.includes("-9999..=9999");
    }
    assert(rejected, `ZonedDateTime.with must reject year ${year}`);
  }

  const nextYear = zoned.with({ year: 2025 });
  assert(nextYear.year === 2025);
  assert(nextYear.month === 3);
  assert(nextYear.day === 9);

  let wrappedMonthRejected = false;
  try {
    Temporal.PlainMonthDay.from("257-01");
  } catch (error: Error) {
    wrappedMonthRejected =
      error.message.includes("Temporal.PlainMonthDay.from{month}") &&
      error.message.includes("1..=12");
  }
  assert(wrappedMonthRejected, "PlainMonthDay.from must reject a wrapping month");

  const constrained = zoned.with({
    month: 65537,
    day: 65540,
    hour: 65560,
    minute: 65540,
    second: 65540,
    millisecond: 65540,
    microsecond: 65540,
    nanosecond: 65540,
  });
  assert(constrained.month === 12);
  assert(constrained.day === 31);
  assert(constrained.hour === 23);
  assert(constrained.minute === 59);
  assert(constrained.second === 59);
  assert(constrained.millisecond === 999);
  assert(constrained.microsecond === 999);
  assert(constrained.nanosecond === 999);
}
