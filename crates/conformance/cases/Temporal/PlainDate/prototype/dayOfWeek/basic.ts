// test262: test/built-ins/Temporal/PlainDate/prototype/dayOfWeek/basic.js
// new Temporal.PlainDate(1976, 11, 14 + i) -> from(ISO string) (no class constructors).

function main(): void {
  for (let i = 1; i <= 7; i++) {
    const plainDate = Temporal.PlainDate.from(`1976-11-${14 + i}`);
    assertSameValue(plainDate.dayOfWeek, i, `${plainDate.toString()} should be on day ${i}`);
  }
}
