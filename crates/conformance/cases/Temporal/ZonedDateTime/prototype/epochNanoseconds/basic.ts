// test262: test/built-ins/Temporal/ZonedDateTime/prototype/epochNanoseconds/basic.js
// new Temporal.ZonedDateTime(ns, "UTC") -> Instant.fromEpochNanoseconds(ns).toZonedDateTimeISO("UTC");
// the `typeof x === "bigint"` assertions are dropped (typeof is a narrowing
// guard only here) — the bigint-literal comparisons already pin the type.

function main(): void {
  const afterEpoch = Temporal.Instant.fromEpochNanoseconds(217175010123456789n).toZonedDateTimeISO("UTC");
  assertSameValue(afterEpoch.epochNanoseconds, 217175010123456789n, "epochNanoseconds post epoch");

  const beforeEpoch = Temporal.Instant.fromEpochNanoseconds(-217175010876543211n).toZonedDateTimeISO("UTC");
  assertSameValue(beforeEpoch.epochNanoseconds, -217175010876543211n, "epochNanoseconds pre epoch");
}
