// test262: test/built-ins/Temporal/Instant/compare/cross-epoch.js

function main(): void {
  const i1 = Temporal.Instant.from("1963-02-13T09:36:29.123456789Z");
  const i2 = Temporal.Instant.from("1976-11-18T15:23:30.123456789Z");
  const i3 = Temporal.Instant.from("1981-12-15T14:34:31.987654321Z");

  // pre epoch equal
  assertSameValue(Temporal.Instant.compare(i1, i1), 0);

  // epoch equal
  assertSameValue(Temporal.Instant.compare(i2, i2), 0);

  // cross epoch smaller/larger
  assertSameValue(Temporal.Instant.compare(i1, i2), -1);

  // cross epoch larger/smaller
  assertSameValue(Temporal.Instant.compare(i2, i1), 1);

  // epoch smaller/larger
  assertSameValue(Temporal.Instant.compare(i2, i3), -1);

  // epoch larger/smaller
  assertSameValue(Temporal.Instant.compare(i3, i2), 1);
}
