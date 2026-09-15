function main(): void {
  const winter = Temporal.ZonedDateTime.from(
    "2024-01-15T12:34:56.123456789-05:00[America/New_York]",
  );
  assert(winter.offset === "-05:00");
  assert(winter.offsetNanoseconds === -18000000000000);
  assert(winter.epochNanoseconds === winter.toInstant().epochNanoseconds);

  const fractional = Temporal.Instant.fromEpochNanoseconds(1999999n);
  const negativeFractional = Temporal.Instant.fromEpochNanoseconds(-1n);
  assert(fractional.epochMilliseconds === 1);
  assert(negativeFractional.epochMilliseconds === -1);
  assert(
    negativeFractional.toZonedDateTimeISO("UTC").epochMilliseconds === -1,
  );

  const spring = Temporal.ZonedDateTime.from(
    "2024-03-10T12:00:00-04:00[America/New_York]",
  );
  const fall = Temporal.ZonedDateTime.from(
    "2024-11-03T12:00:00-05:00[America/New_York]",
  );
  assert(spring.hoursInDay === 23);
  assert(fall.hoursInDay === 25);
  assert(spring.startOfDay().hour === 0);

  const skippedMidnightDate = Temporal.ZonedDateTime.from(
    "2015-10-18T12:00:00-02:00[America/Sao_Paulo]",
  );
  const skippedMidnight = skippedMidnightDate.startOfDay();
  assert(skippedMidnight.hour === 1);
  assert(skippedMidnightDate.hoursInDay === 23);

  const repeatedHour = Temporal.ZonedDateTime.from(
    "2024-11-03T01:30:00-05:00[America/New_York]",
  );
  assert(repeatedHour.with({ minute: 45 }).offset === "-05:00");

  const canonical = winter.withTimeZone("america/new_york");
  assert(canonical.timeZoneId === "America/New_York");
  assert(winter.equals(canonical));

  const kolkata = Temporal.ZonedDateTime.from(
    "2024-01-15T12:00:00+05:30[Asia/Kolkata]",
  );
  const calcutta = kolkata.withTimeZone("Asia/Calcutta");
  assert(calcutta.timeZoneId === "Asia/Calcutta");
  assert(kolkata.equals(calcutta));

  const fixedOffset = Temporal.ZonedDateTime.from(
    "2019-10-29T09:46:38.271986102[-07:00]",
  );
  assert(fixedOffset.timeZoneId === "-07:00");
  assert(fixedOffset.offset === "-07:00");
  assert(fixedOffset.hour === 9);
  assert(fixedOffset.toString().includes("[-07:00]"));

  const annotatedFixedOffset = Temporal.ZonedDateTime.from(
    "2019-10-29T09:46:38[-07:00][u-ca=iso8601]",
  );
  assert(annotatedFixedOffset.timeZoneId === "-07:00");
  assert(annotatedFixedOffset.hour === 9);

  const annotatedNamedZone = Temporal.ZonedDateTime.from(
    "2019-10-29T09:46:38-04:00[America/New_York][u-ca=iso8601]",
  );
  assert(annotatedNamedZone.timeZoneId === "America/New_York");
  assert(annotatedNamedZone.hour === 9);

  const rounded = winter.round({
    smallestUnit: "minute",
    roundingMode: "halfExpand",
  });
  assert(rounded.minute === 35);
  assert(rounded.second === 0);
  assert(winter.round("day").hour === 0);

  let badRoundLabel = false;
  try {
    winter.round({ smallestUnit: "minute", roundingMode: "invalid" });
  } catch (error: Error) {
    badRoundLabel = error.message.includes("Temporal.ZonedDateTime.round");
    assert(!error.message.includes("Temporal.Duration.round"));
  }
  assert(badRoundLabel);

  let badIncrement = false;
  try {
    winter.round({ smallestUnit: "second", roundingIncrement: 7 });
  } catch (error: Error) {
    badIncrement =
      error.message.includes("Temporal.ZonedDateTime.round") &&
      error.message.includes('smallestUnit="second"') &&
      error.message.includes("roundingIncrement=7");
    assert(!error.message.includes("Some("));
    assert(!error.message.includes("None"));
    assert(!error.message.includes("jiff"));
  }
  assert(badIncrement);

  let calendarRoundUnit = false;
  try {
    winter.round({ smallestUnit: "month" });
  } catch (error: Error) {
    calendarRoundUnit =
      error.message.includes("Temporal.ZonedDateTime.round") &&
      error.message.includes("smallestUnit='month'") &&
      error.message.includes("day or smaller") &&
      !error.message.includes("roundingIncrement") &&
      !error.message.includes("largestUnit");
  }
  assert(calendarRoundUnit);

  const positiveOffset = Temporal.ZonedDateTime.from(
    "2024-01-01T12:00:00+01:00[+01:00]",
  );
  assert(positiveOffset.timeZoneId === "+01:00");
  assert(positiveOffset.hour === 12);

  const now = Temporal.Now.zonedDateTimeISO("america/new_york");
  assert(now.timeZoneId === "America/New_York");
  assert(now.epochMilliseconds > 0);
}
