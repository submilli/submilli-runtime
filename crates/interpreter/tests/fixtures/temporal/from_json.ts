function identity<T>(value: T): T {
  return value;
}

function main(): void {
  const instant = Temporal.Instant.fromJson(JSON.parse('"2026-10-15T06:00:00Z"'));
  assert(instant.toString() === "2026-10-15T06:00:00Z");
  assert(identity<Temporal.Instant>(instant).epochMilliseconds === instant.epochMilliseconds);
  assert(Temporal.Instant.fromJson("2026-10-15T09:00:00+03:00[Asia/Jerusalem]").equals(instant));
  assert(Temporal.Instant.fromJson(instant).equals(instant));
  assert(Temporal.ZonedDateTime.fromJson(JSON.parse('{"year":2026,"month":10,"day":15,"hour":9,"timeZone":"Asia/Jerusalem"}')).toInstant().equals(instant));
  assert(Temporal.PlainDateTime.fromJson("2026-10-15T09:30:00").toString() === "2026-10-15T09:30:00");
  assert(Temporal.PlainDate.fromJson(JSON.parse('{"year":2026,"month":10,"day":15}')).toString() === "2026-10-15");
  assert(Temporal.PlainTime.fromJson("09:30:00").toString() === "09:30:00");
  assert(Temporal.PlainYearMonth.fromJson("2026-10").toString() === "2026-10");
  assert(Temporal.PlainMonthDay.fromJson("10-15").toString() === "10-15");
  assert(Temporal.Duration.fromJson(JSON.parse('{"hours":1,"minutes":30}')).toString() === "PT1H30M");
  assert(Temporal.Duration.fromJson("PT90M").toString() === "PT90M");

  let rejected = false;
  try {
    Temporal.Instant.fromJson("not-an-instant");
  } catch (error) {
    rejected = error instanceof RangeError;
  }
  assert(rejected, "invalid text retains the constructor's RangeError");

  rejected = false;
  try {
    Temporal.Instant.fromJson(null);
  } catch (error) {
    rejected = error instanceof TypeError;
  }
  assert(rejected, "invalid input retains the constructor's TypeError");
}
