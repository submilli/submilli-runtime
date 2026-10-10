function value(): number | null { return 40; }
function main(): void {
  const a: number | null = value();
  assert(a! / 10 === 4);
  assert(value()! / 10 === 4);
  const values: (number | null)[] = [40];
  assert(values[0]! / 10 === 4);
  const o: { nums: (number | null)[] } | null = { nums: values };
  assert(o?.nums[0]! / 10 === 4);
  assert(a!! / 10 === 4);
  const keywords: { return: number | null; throw: number | null } = { return: 40, throw: 20 };
  assert(keywords.return! / 10 === 4);
  assert(keywords?.throw! / 10 === 2);
  assert(!/x/.test("y"));
  assert(!!/x/.test("x"));
  const before = 1
  !/x/.test("x");
  assert(before === 1);
  if (true) {}
  !/x/.test("y");
  if (true) {} !/x/.test("y");
  if (true) !/x/.test("y");
}
