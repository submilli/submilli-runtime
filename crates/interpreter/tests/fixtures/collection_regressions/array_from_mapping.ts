function main(): void {
  const strings = Array.from([1, 2], (v: number): string => `${v}`);
  assert(strings[0] === "1" && strings[1] === "2", "type-changing map");
  const chars = Array.from("ab", (v: string): string => v + "!");
  assert(chars[1] === "b!", "string source");
  const pair: [number, number] = [3, 4];
  const tuple = Array.from(pair, (v: number): string => `${v}`);
  assert(tuple[1] === "4", "tuple source");
  const values = new Set<number>();
  values.add(5); values.add(6);
  const iterable = Array.from(values, (v: number): string => `${v}`);
  assert(iterable[0] === "5" && iterable[1] === "6", "iterable source");
  const inferred = Array.from([1, 2], v => `${v}!`);
  assert(inferred[1] === "2!", "contextual callback");
  const explicit = Array.from<number, string>([2], (v: number): string => `${v}`);
  assert(explicit[0] === "2", "explicit type arguments");
  const plain = Array.from([7, 8]);
  assert(plain[0] === 7, "default map");
}
