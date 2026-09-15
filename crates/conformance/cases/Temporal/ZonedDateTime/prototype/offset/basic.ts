// test262: test/built-ins/Temporal/ZonedDateTime/prototype/offset/basic.js
// new Temporal.ZonedDateTime(0n, tz) -> Instant.fromEpochNanoseconds(0n).toZonedDateTimeISO(tz).

function checkOffset(timeZoneIdentifier: string, expectedOffsetString: string, description: string): void {
  const datetime = Temporal.Instant.fromEpochNanoseconds(0n).toZonedDateTimeISO(timeZoneIdentifier);
  assertSameValue(datetime.offset, expectedOffsetString, description);
}

function main(): void {
  checkOffset("UTC", "+00:00", "offset of UTC is +00:00");
  checkOffset("+01:00", "+01:00", "positive offset");
  checkOffset("-05:00", "-05:00", "negative offset");
}
