interface Box { first?: () => number; middle?: number; last?: () => number; }
class All { a: () => number = () => 1; b: () => number = () => 2; }
function f(): number { return 1; }
function main(): void {
  const a: Box = { first: f, last: f };
  const b: Box = { first: f, middle: 2, last: f };
  assert(JSON.stringify(a) === '{}');
  assert(JSON.stringify(b) === '{"middle":2}');
  assert(JSON.stringify(new All()) === '{}');
  const mixed: (number | (() => number) | null)[] = [f, 1, () => 2, null];
  assert(JSON.stringify(mixed) === '[null,1,null,null]');
}
