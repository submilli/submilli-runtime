// expect-error: `Map.get` returns undefined for a missing key
// expect-error: ?? 0
// expect-error: check it first
function main(): void {
  const scores = new Map<string, number>();
  const next = scores.get("missing") + 1;
  let count: number | undefined = undefined;
  count++;
}
