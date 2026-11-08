// expect-error: expected `Exclude<T, undefined> | undefined`, got `T & ({} | null)`
function first<T>(items: T[]): Exclude<T, undefined> | undefined {
  for (const item of items) { if (item !== undefined) { return item; } }
  return undefined;
}
function main(): void {}
