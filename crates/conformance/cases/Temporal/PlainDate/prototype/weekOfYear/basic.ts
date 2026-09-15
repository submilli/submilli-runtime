// test262: test/built-ins/Temporal/PlainDate/prototype/weekOfYear/basic.js
// new Temporal.PlainDate(y, m, d) -> from(ISO string) (no class constructors).

function isoDate(year: number, month: number, day: number): Temporal.PlainDate {
  const mm = month < 10 ? `0${month}` : `${month}`;
  const dd = day < 10 ? `0${day}` : `${day}`;
  return Temporal.PlainDate.from(`${year}-${mm}-${dd}`);
}

function main(): void {
  for (let i = 29; i <= 31; i++) {
    const plainDate = isoDate(1975, 12, i);
    assertSameValue(plainDate.weekOfYear, 1, `${plainDate.toString()} should be in week 1`);
  }
  for (let i = 1; i <= 4; i++) {
    const plainDate = isoDate(1976, 1, i);
    assertSameValue(plainDate.weekOfYear, 1, `${plainDate.toString()} should be in week 1`);
  }
  for (let i = 5; i <= 11; i++) {
    const plainDate = isoDate(1976, 1, i);
    assertSameValue(plainDate.weekOfYear, 2, `${plainDate.toString()} should be in week 2`);
  }
  for (let i = 20; i <= 26; i++) {
    const plainDate = isoDate(1976, 12, i);
    assertSameValue(plainDate.weekOfYear, 52, `${plainDate.toString()} should be in week 52`);
  }
  for (let i = 27; i <= 31; i++) {
    const plainDate = isoDate(1976, 12, i);
    assertSameValue(plainDate.weekOfYear, 53, `${plainDate.toString()} should be in week 53`);
  }
  for (let i = 1; i <= 2; i++) {
    const plainDate = isoDate(1977, 1, i);
    assertSameValue(plainDate.weekOfYear, 53, `${plainDate.toString()} should be in week 53`);
  }
}
