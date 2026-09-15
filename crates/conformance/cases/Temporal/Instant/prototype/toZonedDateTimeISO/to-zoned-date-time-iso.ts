// test262: test/built-ins/Temporal/Instant/prototype/toZonedDateTimeISO/to-zoned-date-time-iso.js
// new Temporal.Instant(ns) -> Temporal.Instant.fromEpochNanoseconds(ns).

function main(): void {
  const inst = Temporal.Instant.fromEpochNanoseconds(1000000000000000000n);

  // time zone parameter UTC
  const zdt = inst.toZonedDateTimeISO("UTC");
  assertSameValue(inst.epochNanoseconds, zdt.epochNanoseconds);
  assertSameValue(zdt.timeZoneId, "UTC");

  // time zone parameter non-UTC
  const zdtNonUTC = inst.toZonedDateTimeISO("-05:00");
  assertSameValue(inst.epochNanoseconds, zdtNonUTC.epochNanoseconds);
  assertSameValue(zdtNonUTC.timeZoneId, "-05:00");
}
