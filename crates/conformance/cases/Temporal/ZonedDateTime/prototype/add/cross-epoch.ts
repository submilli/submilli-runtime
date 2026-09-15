// test262: test/built-ins/Temporal/ZonedDateTime/prototype/add/cross-epoch.js
// new Temporal.ZonedDateTime(ns, "UTC") -> Instant.fromEpochNanoseconds(ns).toZonedDateTimeISO("UTC")
// (no class constructors); assertZonedDateTimesEqual -> equals().

function zdtFromEpochNs(ns: bigint): Temporal.ZonedDateTime {
  return Temporal.Instant.fromEpochNanoseconds(ns).toZonedDateTimeISO("UTC");
}

function main(): void {
  // "1969-12-25T12:23:45.678901234+00:00[UTC]"
  const zdt = zdtFromEpochNs(-560174321098766n);

  // cross epoch in ms
  const one = zdt.subtract({ hours: 240, nanoseconds: 800 });
  const two = zdt.add({ hours: 240, nanoseconds: 800 });
  const three = two.subtract({ hours: 480, nanoseconds: 1600 });
  const four = one.add({ hours: 480, nanoseconds: 1600 });

  // "1969-12-15T12:23:45.678900434+00:00[UTC]"
  assert(one.equals(zdtFromEpochNs(-1424174321099566n)), "subtract pre-epoch");
  // "1970-01-04T12:23:45.678902034+00:00[UTC]"
  assert(two.equals(zdtFromEpochNs(303825678902034n)), "add across the epoch");
  assert(three.equals(one), "round-trip back to one");
  assert(four.equals(two), "round-trip back to two");
}
