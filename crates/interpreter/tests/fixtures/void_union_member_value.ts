function nullable(): void | null { return null; }
function absent(): void | null { return undefined; }
function main(): void {
  assert(nullable() === null, "void union preserves null");
  assert(absent() === undefined, "void union accepts undefined");
}
