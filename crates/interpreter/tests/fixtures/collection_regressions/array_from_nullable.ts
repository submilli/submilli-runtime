function collect(mapper: ((value: number) => string) | null): (number | string)[] {
  return Array.from([1, 2], mapper);
}
function main(): void {
  const identity = Array.from([3, 4], null);
  assert(identity[0] === 3, "explicit null");
  const absent: null = null;
  const copied = Array.from([5], absent);
  assert(copied[0] === 5, "null binding");
  assert(collect(null)[0] === 1, "nullable mapper absent");
  assert(collect((v: number): string => `${v}`)[1] === "2", "nullable mapper present");
}
