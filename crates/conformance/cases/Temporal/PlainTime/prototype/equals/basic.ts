// test262: test/built-ins/Temporal/PlainTime/prototype/equals/basic.js

function main(): void {
  const t1 = Temporal.PlainTime.from("08:44:15.321");
  const t1bis = Temporal.PlainTime.from("08:44:15.321");
  const t2 = Temporal.PlainTime.from("14:23:30.123");
  assertSameValue(t1.equals(t1), true, "same object");
  assertSameValue(t1.equals(t1bis), true, "different object");
  assertSameValue(t1.equals(t2), false, "different times");
}
