function main(): void {
  let mixedConstructorFields = false;
  try {
    new Temporal.Duration({ hours: 12, minutes: -6 });
  } catch (error: Error) {
    mixedConstructorFields =
      error.message.includes("Temporal.Duration") &&
      error.message.includes("hours=12") &&
      error.message.includes("minutes=-6");
    assert(!error.message.includes("jiff"));
  }
  assert(mixedConstructorFields);

  let mixedWithFields = false;
  try {
    Temporal.Duration.from("-PT12H6M").with({ hours: 12 });
  } catch (error: Error) {
    mixedWithFields =
      error.message.includes("Temporal.Duration.with") &&
      error.message.includes("hours=12") &&
      error.message.includes("minutes=-6");
    assert(!error.message.includes("jiff"));
  }
  assert(mixedWithFields);

  let fieldRange = false;
  try {
    Temporal.Duration.from({ years: 4294967296 });
  } catch (error: Error) {
    fieldRange =
      error.message.includes("Temporal.Duration.from") &&
      error.message.includes("years=4294967296") &&
      error.message.includes("-19998..=19998");
    assert(!error.message.includes("jiff"));
  }
  assert(fieldRange);

  const largeNanoseconds = Temporal.Duration.from({
    nanoseconds: 9000000000000000000,
  });
  assert(largeNanoseconds.nanoseconds === 9000000000000000000);

  let nanosecondsOverflow = false;
  try {
    Temporal.Duration.from({
      nanoseconds: 9007199254740992000000000,
    });
  } catch (error: Error) {
    nanosecondsOverflow =
      error.message.includes("Temporal.Duration.from") &&
      error.message.includes("nanoseconds=") &&
      error.message.includes("2^53-second limit");
  }
  assert(nanosecondsOverflow);

  let missingRoundUnit = false;
  try {
    Temporal.Duration.from("PT1H30M45.5S").round({
      roundingMode: "ceil",
    });
  } catch (error: Error) {
    missingRoundUnit = error.message.includes(
      "Temporal.Duration.round: one of smallestUnit or largestUnit is required",
    );
  }
  assert(missingRoundUnit);

  let sanitizedRoundError = false;
  try {
    Temporal.Duration.from("P1M").round({ smallestUnit: "day" });
  } catch (error: Error) {
    sanitizedRoundError =
      error.message.includes("Temporal.Duration.round") &&
      error.message.includes("relativeTo");
    assert(!error.message.includes("jiff"));
    assert(!error.message.includes("Some("));
    assert(!error.message.includes("None"));
  }
  assert(sanitizedRoundError);

  const oneDay = Temporal.Duration.from("P1D");
  const hours24 = Temporal.Duration.from("PT24H");
  const twoDays = oneDay.add(hours24);
  assert(twoDays.days === 2);
  assert(twoDays.hours === 0);
  const subtractedDay = twoDays.subtract(oneDay);
  assert(subtractedDay.days === 1);
  assert(subtractedDay.hours === 0);
  const roundedDay = hours24.round({ largestUnit: "day" });
  assert(roundedDay.days === 1);
  assert(roundedDay.hours === 0);
  assert(oneDay.total("hours") === 24);
  assert(Temporal.Duration.compare(oneDay, hours24) === 0);

  const hours48 = Temporal.Duration.from("PT48H");
  assert(hours48.total({ unit: "days" }) === 2);

  const blankDuration = Temporal.Duration.from({});
  assert(blankDuration.total({ unit: "days" }) === 0);
  assert(blankDuration.total("days") === 0);
  assert(blankDuration.total({ unit: "hours" }) === 0);
  assert(blankDuration.total({ unit: "seconds" }) === 0);

  let blankYearsRequireAnchor = false;
  try {
    blankDuration.total({ unit: "years" });
  } catch (error: Error) {
    blankYearsRequireAnchor =
      error.message.includes("Temporal.Duration.total") &&
      error.message.includes("relativeTo anchor");
  }
  assert(blankYearsRequireAnchor);

  let blankMonthsRequireAnchor = false;
  try {
    blankDuration.total({ unit: "months" });
  } catch (error: Error) {
    blankMonthsRequireAnchor = error.message.includes("relativeTo anchor");
  }
  assert(blankMonthsRequireAnchor);

  let blankWeeksRequireAnchor = false;
  try {
    blankDuration.total({ unit: "weeks" });
  } catch (error: Error) {
    blankWeeksRequireAnchor = error.message.includes("relativeTo anchor");
  }
  assert(blankWeeksRequireAnchor);

  const blankRelativeDate = Temporal.PlainDate.from("2024-01-01");
  assert(
    blankDuration.total({
      unit: "days",
      relativeTo: blankRelativeDate,
    }) === 0,
  );
  assert(
    blankDuration.total({
      unit: "years",
      relativeTo: blankRelativeDate,
    }) === 0,
  );
  assert(
    blankDuration.total({
      unit: "days",
      relativeTo: Temporal.ZonedDateTime.from(
        "2024-01-01T00:00:00+00:00[UTC]",
      ),
    }) === 0,
  );
  assert(oneDay.subtract(oneDay).total({ unit: "days" }) === 0);

  const oneWeek = Temporal.Duration.from("P1W");
  let weekAddRequiresAnchor = false;
  try {
    oneWeek.add({ days: 1 });
  } catch (error: Error) {
    weekAddRequiresAnchor =
      error.message.includes("Temporal.Duration.add") &&
      error.message.includes("weeks") &&
      error.message.includes("relativeTo") &&
      !error.message.includes("smaller duration values");
  }
  assert(weekAddRequiresAnchor);

  let weekSubtractRequiresAnchor = false;
  try {
    oneWeek.subtract({ days: 1 });
  } catch (error: Error) {
    weekSubtractRequiresAnchor =
      error.message.includes("Temporal.Duration.subtract") &&
      error.message.includes("weeks") &&
      error.message.includes("relativeTo");
  }
  assert(weekSubtractRequiresAnchor);

  let weekRoundRequiresAnchor = false;
  try {
    oneWeek.round({ largestUnit: "day" });
  } catch (error: Error) {
    weekRoundRequiresAnchor =
      error.message.includes("Temporal.Duration.round") &&
      error.message.includes("weeks") &&
      error.message.includes("relativeTo");
  }
  assert(weekRoundRequiresAnchor);

  let weekCompareRequiresAnchor = false;
  try {
    Temporal.Duration.compare(oneWeek, { days: 7 });
  } catch (error: Error) {
    weekCompareRequiresAnchor =
      error.message.includes("Temporal.Duration.compare") &&
      error.message.includes("weeks") &&
      error.message.includes("relativeTo");
  }
  assert(weekCompareRequiresAnchor);

  let weekTotalRequiresAnchor = false;
  try {
    oneWeek.total({ unit: "days" });
  } catch (error: Error) {
    weekTotalRequiresAnchor =
      error.message.includes("Temporal.Duration.total") &&
      error.message.includes("years, months, and weeks") &&
      error.message.includes(
        "balance those units relative to a PlainDate or ZonedDateTime first",
      );
  }
  assert(weekTotalRequiresAnchor);

  let monthTotalRequiresAnchor = false;
  try {
    Temporal.Duration.from({ months: 1 }).total({ unit: "days" });
  } catch (error: Error) {
    monthTotalRequiresAnchor = error.message.includes(
      "balance those units relative to a PlainDate or ZonedDateTime first",
    );
  }
  assert(monthTotalRequiresAnchor);

  let yearTotalRequiresAnchor = false;
  try {
    Temporal.Duration.from({ years: 1 }).total({ unit: "days" });
  } catch (error: Error) {
    yearTotalRequiresAnchor = error.message.includes(
      "balance those units relative to a PlainDate or ZonedDateTime first",
    );
  }
  assert(yearTotalRequiresAnchor);

  assert(
    Temporal.Duration.from({ months: 1 }).total({
      unit: "days",
      relativeTo: Temporal.PlainDate.from("2024-02-01"),
    }) === 29,
  );

  let anchoredCompareRange = false;
  try {
    Temporal.Duration.compare(
      { years: 19998 },
      { years: 1 },
      { relativeTo: Temporal.PlainDate.from("2024-02-01") },
    );
  } catch (error: Error) {
    anchoredCompareRange =
      error.message.includes("Temporal.Duration.compare") &&
      error.message.includes("supplied relativeTo") &&
      error.message.includes("outside the supported range") &&
      !error.message.includes("pass a relativeTo");
  }
  assert(anchoredCompareRange);

  const instant = Temporal.Instant.from("2024-01-01T00:00:00Z");
  let instantRoundError = false;
  try {
    instant.round({ smallestUnit: "second", roundingIncrement: 7 });
  } catch (error: Error) {
    instantRoundError =
      error.message.includes("Temporal.Instant.round") &&
      error.message.includes('smallestUnit="second"') &&
      error.message.includes("roundingIncrement=7") &&
      error.message.includes("divides evenly");
    assert(!error.message.includes("jiff"));
  }
  assert(instantRoundError);

  let instantUntilError = false;
  try {
    instant.until(instant, { smallestUnit: "month" });
  } catch (error: Error) {
    instantUntilError =
      error.message.includes("Temporal.Instant.until") &&
      error.message.includes('smallestUnit="month"');
    assert(!error.message.includes("jiff"));
  }
  assert(instantUntilError);

  let instantSinceError = false;
  try {
    instant.since(instant, { largestUnit: "month" });
  } catch (error: Error) {
    instantSinceError =
      error.message.includes("Temporal.Instant.since") &&
      error.message.includes('largestUnit="month"');
    assert(!error.message.includes("jiff"));
  }
  assert(instantSinceError);

  const maxDate = Temporal.PlainDate.from("9999-12-31");
  let plainDateAddError = false;
  try {
    maxDate.add({ days: 1 });
  } catch (error: Error) {
    plainDateAddError =
      error.message.includes("Temporal.PlainDate.add") &&
      error.message.includes("P1D");
    assert(!error.message.includes("jiff"));
  }
  assert(plainDateAddError);

  let plainDateSubtractError = false;
  try {
    maxDate.subtract({ days: -1 });
  } catch (error: Error) {
    plainDateSubtractError =
      error.message.includes("Temporal.PlainDate.subtract") &&
      error.message.includes("-P1D");
    assert(!error.message.includes("jiff"));
  }
  assert(plainDateSubtractError);

  const maxDateTime = Temporal.PlainDateTime.from("9999-12-31T23:59:59");
  let plainDateTimeAddError = false;
  try {
    maxDateTime.add({ seconds: 1 });
  } catch (error: Error) {
    plainDateTimeAddError =
      error.message.includes("Temporal.PlainDateTime.add") &&
      error.message.includes("PT1S");
    assert(!error.message.includes("jiff"));
  }
  assert(plainDateTimeAddError);

  let plainDateTimeSubtractError = false;
  try {
    maxDateTime.subtract({ seconds: -1 });
  } catch (error: Error) {
    plainDateTimeSubtractError =
      error.message.includes("Temporal.PlainDateTime.subtract") &&
      error.message.includes("-PT1S");
    assert(!error.message.includes("jiff"));
  }
  assert(plainDateTimeSubtractError);

  const zdt = Temporal.ZonedDateTime.from(
    "2024-01-15T12:00:00-05:00[America/New_York]",
  );
  let subtractLabel = false;
  try {
    zdt.subtract({ hours: 1, minutes: -1 });
  } catch (error: Error) {
    subtractLabel =
      error.message.includes("Temporal.ZonedDateTime.subtract") &&
      error.message.includes("hours=1") &&
      error.message.includes("minutes=-1");
    assert(!error.message.includes("jiff"));
  }
  assert(subtractLabel);

  let untilLabel = false;
  try {
    zdt.until(zdt, { roundingMode: "invalid" });
  } catch (error: Error) {
    untilLabel =
      error.message.includes("Temporal.ZonedDateTime.until") &&
      error.message.includes('roundingMode "invalid"');
    assert(!error.message.includes("ZonedDateTime difference"));
    assert(!error.message.includes("jiff"));
  }
  assert(untilLabel);
}
