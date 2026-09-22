function byte(value: Uint8Array | null): number | null { return value?.[0]; }
function item(value: [number, string] | null): string | null { return value?.[1]; }
function main(): void {
  assert(byte(new Uint8Array([42])) === 42, "byte");
  assert(byte(null) === null, "null byte");
  assert(item([1, "two"]) === "two", "tuple position");
  assert(item(null) === null, "null tuple");
  let calls = 0;
  const index = (): number => { calls++; return 0; };
  const none: Uint8Array | null = null;
  assert(none?.[index()] === null && calls === 0, "skip index");
}
