// expect-error: `JSON.stringify` only accepts `null` as its replacer argument
function main(): void {
  JSON.stringify({ a: 1 }, (key: string, value: unknown): unknown => value, 2);
}
