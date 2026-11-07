interface Point {
  x: number;
  y: number;
}
function main(): void {
  const p: Point = { x: 1, y: 2 };
  const copied = { ...p };
  assert(copied.x === 1 && copied.y === 2, "interface annotation spreads");
}
