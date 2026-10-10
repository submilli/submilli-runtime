// The first clause's `Math` shadows the built-in namespace across the whole
// switch body, so `default`, entered directly, reaches it before its
// declaration has run (TS2454).
// expect-error: `Math` is declared in another `case` clause
// expect-error-count: 1
function pick(kind: number): number {
  switch (kind) {
    case 1:
      const Math = { floor: (x: number): number => x * 100 };
      return Math.floor(1.5);
    default:
      return Math.floor(2.5);
  }
}

function main(): void {
  console.log(String(pick(2)));
}
