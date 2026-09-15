// test262: test/built-ins/Temporal/ZonedDateTime/prototype/startOfDay/basic.js
// new Temporal.ZonedDateTime(ns, "UTC") -> Instant.fromEpochNanoseconds(ns).toZonedDateTimeISO("UTC").

function main(): void {
  const instance = Temporal.Instant.fromEpochNanoseconds(10000n * 86400000000000n + 7272123456789n).toZonedDateTimeISO("UTC");
  const result = instance.startOfDay();
  assertSameValue(result.epochNanoseconds, 10000n * 86400000000000n);
}
