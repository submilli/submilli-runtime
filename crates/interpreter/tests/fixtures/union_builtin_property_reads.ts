function size(value: string[] | Uint8Array): number {
  return value.length;
}
function collectionSize(value: Map<string, number> | Set<string>): number {
  return value.size;
}
function measure<T extends { length: number }>(value: T | string[]): number {
  return value.length;
}
function main(): void {
  checkBoundaries();
  checkTuples();
  assert(size(["a", "b"]) === 2);
  assert(size(new Uint8Array(3)) === 3);
  assert(collectionSize(new Map<string, number>([["a", 1]])) === 1);
  assert(collectionSize(new Set<string>(["a", "b"])) === 2);
  assert(measure({ length: 3 }) === 3);
  assert(measure(["a"]) === 1);
}

type LengthValue = readonly string[] | Uint8Array | string;
function optionalLength(value: LengthValue | null | undefined): number | undefined {
  return value?.length;
}
function optionalSize(value: Map<string, number> | Set<string> | undefined): number | undefined {
  return value?.size;
}
function mixedLength(value: string[] | { length: number }): number {
  return value.length;
}
function checkBoundaries(): void {
  assert(optionalLength(undefined) === undefined);
  assert(optionalLength(null) === undefined);
  assert(optionalLength([]) === 0);
  assert(optionalLength(new Uint8Array(0)) === 0);
  assert(optionalLength("\ud800") === 1);
  assert(optionalSize(undefined) === undefined);
  assert(optionalSize(new Map<string, number>()) === 0);
  assert(optionalSize(new Set<string>(["a"])) === 1);
  assert(mixedLength({ length: 7 }) === 7);
  assert(mixedLength(["a"]) === 1);
  const measured = new Measured();
  assert(classLength(measured) === 9);
  assert(measured.reads === 1);
  assert(classLength(["a", "b"]) === 2);
  assert(nextLengthValue().length === 2);
  assert(evaluations === 1);
}

class Measured {
  reads: number = 0;
  get length(): number {
    this.reads++;
    return 9;
  }
}
function classLength(value: string[] | Measured): number {
  return value.length;
}
let evaluations: number = 0;
function nextLengthValue(): string[] | Uint8Array {
  evaluations++;
  return ["a", "b"];
}

function tupleLength(value: [string, string] | { length: 1 }): 1 | 2 {
  return value.length;
}
function optionalTupleLength(value: readonly [string, string] | { length: 1 } | undefined): 1 | 2 | undefined {
  return value?.length;
}
function checkTuples(): void {
  assert(tupleLength(["a", "b"]) === 2);
  assert(tupleLength({ length: 1 }) === 1);
  assert(optionalTupleLength(["a", "b"]) === 2);
  assert(optionalTupleLength({ length: 1 }) === 1);
  assert(optionalTupleLength(undefined) === undefined);
}
