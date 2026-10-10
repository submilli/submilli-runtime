function main(): void {
  const bytes = new Uint8Array([255, 0]);
  const old = bytes[0]++;
  assert(old === 255 && bytes[0] === 0, "increment wraps");
  const zero = bytes[1]--;
  assert(zero === 0 && bytes[1] === 255, "decrement wraps");
  bytes[1] = 0;
  bytes[1]--;
  assert(bytes[1] === 255, "statement decrement wraps");
  let calls = 0;
  const receiver = (): Uint8Array => { calls++; return bytes; };
  const index = (): number => { calls++; return 0; };
  const next = receiver()[index()]++;
  assert(next === 0 && bytes[0] === 1 && calls === 2, "evaluate once");
}
