function take(o: {
  a: number
  b: string
  nested: {
    x: number
    y: number
  }
}): number {
  return o.a + o.nested.x + o.nested.y;
}
function main(): void {
  assert(take({ a: 1, b: "x", nested: { x: 2, y: 3 } }) === 6);
  const value = (true ? { a: 1 } : {
    a: 2,
  });
  assert(value.a === 1);
  const nested = ({ a: {
    x: 2,
    y: 3
  } });
  assert(nested.a.y === 3);
}
