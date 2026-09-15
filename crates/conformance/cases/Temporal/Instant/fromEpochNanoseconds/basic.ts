// test262: test/built-ins/Temporal/Instant/fromEpochNanoseconds/basic.js

function main(): void {
  const afterEpoch = Temporal.Instant.fromEpochNanoseconds(217175010123456789n);
  assertSameValue(afterEpoch.epochNanoseconds, 217175010123456789n, "fromEpochNanoseconds post epoch");

  const beforeEpoch = Temporal.Instant.fromEpochNanoseconds(-217175010876543211n);
  assertSameValue(beforeEpoch.epochNanoseconds, -217175010876543211n, "fromEpochNanoseconds pre epoch");
}
