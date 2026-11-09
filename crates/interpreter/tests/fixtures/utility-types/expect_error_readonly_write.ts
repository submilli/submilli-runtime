// expect-error: cannot assign to readonly property `x` on `Readonly<Point>`
type Point = { x: number };
function main(): void { const p: Readonly<Point> = { x: 1 }; p.x = 2; }
