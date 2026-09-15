// test262: test/built-ins/Temporal/Instant/limits.js
// expect-fail: representable Instant range is narrower than the standard ±8.64e21 ns, and the six-digit extended-year ISO forms ("-271821-04-20", "+275760-09-13") are not parsed
// new Temporal.Instant(ns) -> Temporal.Instant.fromEpochNanoseconds(ns) (no class constructors).

function main(): void {
  const limit: bigint = 8640000000000000000000n;
  assertThrows((): void => {
    Temporal.Instant.fromEpochNanoseconds(-limit - 1n);
  }, "below minimum");
  assertThrows((): void => {
    Temporal.Instant.fromEpochNanoseconds(limit + 1n);
  }, "above maximum");
  assert(
    Temporal.Instant.fromEpochNanoseconds(-limit).equals(Temporal.Instant.from("-271821-04-20T00:00:00Z")),
    "minimum instant",
  );
  assert(
    Temporal.Instant.fromEpochNanoseconds(limit).equals(Temporal.Instant.from("+275760-09-13T00:00:00Z")),
    "maximum instant",
  );
}
