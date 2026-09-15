// expect-error: cannot assign to readonly property `x`
export function main(): string {
  const point: { readonly x: number; y: number } = { x: 1, y: 2 };
  point.x = 5;
  return point.y.toString();
}
