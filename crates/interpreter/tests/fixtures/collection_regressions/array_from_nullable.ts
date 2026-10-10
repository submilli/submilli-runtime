function collect(mapper: ((value: number) => string) | undefined): (number | string)[] {
  return Array.from([1, 2], mapper);
}
function main(): void {
  const identity = Array.from([3, 4], undefined);
  assert(identity[0] === 3, "explicit undefined");
  const absent: undefined = undefined;
  const copied = Array.from([5], absent);
  assert(copied[0] === 5, "undefined binding");
  assert(collect(undefined)[0] === 1, "optional mapper absent");
  assert(collect((v: number): string => `${v}`)[1] === "2", "optional mapper present");
}
