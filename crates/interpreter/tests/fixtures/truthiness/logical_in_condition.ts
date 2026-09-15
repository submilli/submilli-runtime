function bothLen(a: string | null, b: string | null): number {
  if (a && b) {
    return a.length + b.length;
  }
  return -1;
}

function bothLenExplicit(x: number[] | null, y: number[] | null): number {
  if (x !== null && y !== null) {
    return x.length + y.length;
  }
  return -1;
}

function main(): void {
  assert(bothLen("ab", "c") === 3, "both truthy narrows both to string");
  assert(bothLen("", "c") === -1, "empty string fails the joint condition");
  assert(bothLen(null, "c") === -1, "null fails the joint condition");
  assert(bothLenExplicit([1], [2, 3]) === 3, "explicit null checks still narrow");
  assert(bothLenExplicit(null, [1]) === -1, "explicit null check fails on null");
}
