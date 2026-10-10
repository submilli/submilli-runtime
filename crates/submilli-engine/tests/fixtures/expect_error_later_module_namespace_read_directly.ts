// Top-level code that runs above a later `const Math` reads it before its
// declaration (TS2448), not the built-in namespace it shadows.
// expect-error: unresolved identifier `Math`
// expect-error-count: 1
const floored: number = Math.floor(2.5);
const Math: { floor: (x: number) => number } = {
  floor: (x: number): number => x * 10,
};

export function main(): void {
  console.log(String(floored));
}
