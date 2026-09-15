// test262: test/built-ins/Temporal/Instant/prototype/toString/basic.js
// new Temporal.Instant(ns) -> Temporal.Instant.fromEpochNanoseconds(ns) (no class constructors).

function main(): void {
  const afterEpoch = Temporal.Instant.fromEpochNanoseconds(217175010123456789n);
  assertSameValue(afterEpoch.toString(), "1976-11-18T14:23:30.123456789Z", "basic toString() after epoch");

  const beforeEpoch = Temporal.Instant.fromEpochNanoseconds(-217175010876543211n);
  assertSameValue(beforeEpoch.toString(), "1963-02-13T09:36:29.123456789Z", "basic toString() before epoch");
}
