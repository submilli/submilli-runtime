// test262: test/built-ins/Temporal/Instant/prototype/equals/basic.js
// new Temporal.Instant(ns) -> Temporal.Instant.fromEpochNanoseconds(ns) (no class constructors).

function main(): void {
  const inst1 = Temporal.Instant.fromEpochNanoseconds(1234567890123456789n);
  const inst2 = Temporal.Instant.fromEpochNanoseconds(1234567890123456000n);
  const inst3 = Temporal.Instant.fromEpochNanoseconds(1234567890123456000n);
  assert(!inst1.equals(inst2));
  assert(inst2.equals(inst3));
}
