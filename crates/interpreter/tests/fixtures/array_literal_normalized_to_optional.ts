// A normalized array of object literals fits a type whose field is optional:
// the field the normalization adds (`b?: null`, tsc's `b?: undefined`) is
// absent, which any optional field allows. Writes through the wider type land
// on the same objects.
type Row = { a: number; b?: number };

function total(rows: Row[]): number {
  let sum = 0;
  for (const r of rows) sum += r.a + (r.b ?? 0);
  return sum;
}

function main(): void {
  const rows = [{ a: 1 }, { a: 2, b: 10 }];
  const one: Row = rows[0];
  assert(total(rows) === 13 && one.a === 1, "passes as Row[] and Row");

  one.b = 5;
  assert(rows[0].b === 5, "a write through Row reaches the element");

  const asRows: Row[] = rows;
  asRows.push({ a: 3, b: 1 });
  assert(total(rows) === 22, "a push through Row[] reaches the array");
}
