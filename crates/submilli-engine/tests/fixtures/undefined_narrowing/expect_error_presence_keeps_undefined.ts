// expect-error: undefined
function read(value: { count?: number }): number {
  if ("count" in value) { return value.count; }
  return 0;
}
function main(): void { read({}); }
