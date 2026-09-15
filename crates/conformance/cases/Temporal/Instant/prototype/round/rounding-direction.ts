// test262: test/built-ins/Temporal/Instant/prototype/round/rounding-direction.js
// new Temporal.Instant(ns) -> Temporal.Instant.fromEpochNanoseconds(ns) (no class constructors).

function main(): void {
  const instance = Temporal.Instant.fromEpochNanoseconds(-65261246399500000000n); // -000099-12-15T12:00:00.5Z
  assertSameValue(
    instance.round({ smallestUnit: "second", roundingMode: "floor" }).epochNanoseconds,
    -65261246400000000000n,
    "Rounding down is towards the Big Bang, not the epoch or 1 BCE (roundingMode floor)",
  );
  assertSameValue(
    instance.round({ smallestUnit: "second", roundingMode: "trunc" }).epochNanoseconds,
    -65261246400000000000n,
    "Rounding down is towards the Big Bang, not the epoch or 1 BCE (roundingMode trunc)",
  );
  assertSameValue(
    instance.round({ smallestUnit: "second", roundingMode: "ceil" }).epochNanoseconds,
    -65261246399000000000n,
    "Rounding up is away from the Big Bang, not the epoch or 1 BCE (roundingMode ceil)",
  );
  assertSameValue(
    instance.round({ smallestUnit: "second", roundingMode: "halfExpand" }).epochNanoseconds,
    -65261246399000000000n,
    "Rounding up is away from the Big Bang, not the epoch or 1 BCE (roundingMode halfExpand)",
  );
  assertSameValue(
    instance.round({ smallestUnit: "second" }).epochNanoseconds,
    -65261246399000000000n,
    "The default rounding mode is halfExpand",
  );
}
