// expect-error: `Record<K, V>` is not supported
// expect-error: use `Map<K, V>` instead

function main(): void {
  const results: Record<string, number> = new Map<string, number>();
}
