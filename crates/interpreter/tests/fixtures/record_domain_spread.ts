// Existing Submilli behavior retains spread signatures that TypeScript drops.
function main(): void {
  const a: Record<`a${string}`, boolean> = { a1: true };
  const b: Record<`b${string}`, boolean> = { b1: false };
  const spread = { ...a, ...b };
  assert(spread.a1 === true && spread.b1 === false, "same-value spread preserves both domains");
}
