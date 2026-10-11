// expect-error: `JSON.stringify` only accepts `null` or `undefined` as its replacer argument
function main(): void {
  JSON.stringify({ a: 1 }, (key: string, value: unknown): unknown => value, 2);
}
