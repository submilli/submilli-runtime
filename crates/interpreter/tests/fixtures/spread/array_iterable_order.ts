function source(log: string[]): Set<number> {
  log.push("source");
  return new Set<number>([1, 2]);
}

function after(log: string[], set: Set<number>): number {
  log.push("after");
  set.clear();
  return 3;
}

function copyWithShadowedArray(Array: number, set: Set<number>): number[] {
  assert(Array === 7, "local Array binding");
  return [...set];
}

function copyUnion(value: number[] | Set<number> | string): (number | string)[] {
  return [...value];
}

function main(): void {
  const log: string[] = [];
  const set = source(log);
  const result = [0, ...set, after(log, set), ...source(log)];
  assert(result.join(",") === "0,1,2,3,1,2", "snapshot before subsequent mutation");
  assert(log.join(",") === "source,after,source", "source evaluated once in order");
  assert(copyWithShadowedArray(7, new Set<number>([8]))[0] === 8, "intrinsic collection");
  assert(copyUnion([4])[0] === 4, "union array");
  assert(copyUnion(new Set<number>([5]))[0] === 5, "union Set");
  assert(copyUnion("😀")[0] === "😀", "union string");
  const mixed: [number, number] | number[] = [...new Set<number>([1, 2, 3])];
  assert(mixed.length === 3, "tuple-or-array context accepts iterable");
}
