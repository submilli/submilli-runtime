class Box { a: () => number = () => 1; b: number = 2; z: () => number = () => 3; }
function main(): void {
  const f = (n: number): number => n + 1;
  assert(JSON.stringify([f]) === "[null]");
  assert(JSON.stringify({ f: f, g: 2 }) === '{"g":2}');
  assert(JSON.stringify({ f: f }) === '{}');
  const erased: unknown = f;
  assert(JSON.stringify({ a: erased, b: 2, c: erased }) === '{"b":2}');
  assert(JSON.stringify([erased, null, 2]) === '[null,null,2]');
  assert(JSON.stringify(new Box()) === '{"b":2}');
}
