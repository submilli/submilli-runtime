// test262: test/built-ins/Temporal/Instant/prototype/add/cross-epoch.js

function main(): void {
  const inst = Temporal.Instant.from("1969-12-25T12:23:45.678901234Z");

  // cross epoch in ms
  const one = inst.subtract({ hours: 240, nanoseconds: 800 });
  const two = inst.add({ hours: 240, nanoseconds: 800 });
  const three = two.subtract({ hours: 480, nanoseconds: 1600 });
  const four = one.add({ hours: 480, nanoseconds: 1600 });
  assert(
    one.equals(Temporal.Instant.from("1969-12-15T12:23:45.678900434Z")),
    `(${inst.toString()}).subtract({ hours: 240, nanoseconds: 800 }) = ${one.toString()}`,
  );
  assert(
    two.equals(Temporal.Instant.from("1970-01-04T12:23:45.678902034Z")),
    `(${inst.toString()}).add({ hours: 240, nanoseconds: 800 }) = ${two.toString()}`,
  );
  assert(three.equals(one), `(${two.toString()}).subtract({ hours: 480, nanoseconds: 1600 }) = ${one.toString()}`);
  assert(four.equals(two), `(${one.toString()}).add({ hours: 480, nanoseconds: 1600 }) = ${two.toString()}`);
}
