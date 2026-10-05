// Function elements no one type fits stay rejected: different parameter types
// or return types would need a union of function types (spec §1.2,
// homogeneous arrays).
// expect-error: expected `(arg0: number) => number` (matching first element), got `(arg0: string) => number`
// expect-error: expected `(arg0: number) => number` (matching first element), got `() => string`
// expect-error: expected `(arg0: number, arg1: number) => number` (matching the elements before it), got `() => string`
// expect-error-count: 3
function main(): void {
  const byParam = [(x: number) => x, (x: string) => x.length];
  const byResult = [(x: number) => x, (): string => "s"];
  const afterWidening = [(x: number) => x, (x: number, y: number) => x * y, (): string => "s"];
}
