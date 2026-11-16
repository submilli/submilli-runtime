interface Sized { size: number }
interface Pattern { source: string; lastIndex: number }
interface Length { length: number }
function size(value: Sized): number { return value.size; }
function source(value: Pattern): string { return value.source; }
function increment(value: Pattern): number { return value.lastIndex++; }
function advance(value: Pattern): void { value.lastIndex += 2; }
function optionalSize(value: Sized | undefined): number | undefined { return value?.size; }
function main(): void {
  const map: Sized = new Map<string, number>([["a", 1]]);
  assert(size(map) === 1);
  assert(optionalSize(map) === 1);
  assert(optionalSize(undefined) === undefined);
  assert(size(new Set<number>([1, 2])) === 2);
  assert(size({ size: 3 }) === 3);
  const pattern: Pattern = /abc/g;
  assert(source(pattern) === "abc");
  advance(pattern);
  assert(increment(pattern) === 2);
  assert(pattern.lastIndex === 3);
  const array: Length = [1, 2];
  array.length--;
  assert(array.length === 1);
  const text: Length = "abcd";
  assert(text.length === 4);
}
