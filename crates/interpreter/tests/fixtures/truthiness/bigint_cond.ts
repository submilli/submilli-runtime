function truthy(b: bigint): boolean {
  return !!b;
}

function main(): void {
  assert(!truthy(0n), "0n is falsy");
  assert(truthy(1n), "1n is truthy");
  assert(truthy(-1n), "-1n is truthy");
  const r: string = 0n ? "a" : "b";
  assert(r === "b", "ternary on 0n");
}
