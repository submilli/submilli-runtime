// Temporal's field-bag methods are declared twice: the interface `MethodSig`
// types the bag as `Temporal.PlainDateFields`, while the host wrapper takes a
// structural object. Passing a value whose static type is the interface (not an
// inline literal, which is already structurally typed) has to coerce into the
// wrapper's real slot.

function main(): void {
  const fields: Temporal.PlainDateFields = { year: 2021 };
  const d = Temporal.PlainDate.from("2020-01-02");
  assert(d.with(fields).toString() === "2021-01-02", "with() on an interface-typed bag");

  const time: Temporal.PlainTimeFields = { hour: 5 };
  const t = Temporal.PlainTime.from("01:02:03");
  assert(t.with(time).toString() === "05:02:03", "PlainTime#with");
}
