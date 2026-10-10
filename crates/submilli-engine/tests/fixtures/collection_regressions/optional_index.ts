function byte(value: Uint8Array | null): number | undefined { return value?.[0]; }
function item(value: [number, string] | null): string | undefined { return value?.[1]; }
function main(): void {
  assert(byte(new Uint8Array([42])) === 42, "byte");
  assert(byte(null) === undefined, "null byte");
  assert(item([1, "two"]) === "two", "tuple position");
  assert(item(null) === undefined, "null tuple");
  let calls = 0;
  const index = (): number => { calls++; return 0; };
  const none: Uint8Array | null = null as Uint8Array | null;
  assert(none?.[index()] === undefined && calls === 0, "skip index");
}
