function read(value: { count: number } | null = null): number {
  if (value) { return value.count; }
  return 0;
}
function inverse(value: { count: number } | null = null): number {
  if (!value) { return 0; }
  return value.count;
}
function main(): void {
  assert(read() === 0);
  assert(read(null) === 0);
  assert(read({ count: 7 }) === 7);
  assert(inverse() === 0);
  assert(inverse(null) === 0);
  assert(inverse({ count: 7 }) === 7);
}
