// Function elements no one type fits stay rejected: different parameter types
// or return types would need a union of function types (spec §1.2,
// homogeneous arrays).
// expect-error: expected `(arg0: number) => number` (matching first element), got `(arg0: string) => number`
// expect-error: expected `(arg0: number) => number` (matching first element), got `() => string`
// expect-error-count: 2
function main(): void {
  const byParam = [(x: number) => x, (x: string) => x.length];
  const byResult = [(x: number) => x, (): string => "s"];
}
