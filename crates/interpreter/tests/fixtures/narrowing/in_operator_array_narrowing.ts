function read(array: number[]): number {
  if ("0" in array) return array[0];
  return -1;
}

function lengthGuard(array: number[]): number {
  if ("length" in array) return array[0];
  return -1;
}

function union(value: number[] | { other: string }): string {
  if ("other" in value) return value.other;
  return String(value[0]);
}

function unionLength(value: number[] | { other: string }): string {
  if ("length" in value) return String(value[0]);
  return value.other;
}

function unionIndex(value: number[] | { other: string }): number {
  if (0 in value) return value[0];
  return -1;
}

function main(): void {
  assert(read([10, 20]) === 10);
  assert(read([]) === -1);
  assert(lengthGuard([30]) === 30);
  assert(union([40]) === "40");
  assert(union({ other: "object" }) === "object");
  assert(unionLength([50]) === "50");
  assert(unionLength({ other: "object" }) === "object");
  assert(unionIndex([60]) === 60);
  assert(unionIndex({ other: "object" }) === -1);
  const array = [1, 2];
  if (!("99" in array)) assert(array[0] === 1);
  assert(array[1] === 2);
  const tuple: [number, string] = [1, "two"];
  if ("1" in tuple) assert(tuple[1] === "two");
  const match = /a/.exec("a");
  if (match !== null) {
    assert("index" in match);
    assert("input" in match);
    assert("groups" in match);
  }
}
