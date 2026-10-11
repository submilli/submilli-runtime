// expect-error: undefined
function read(value: { name: string } | undefined): string {
  if (value?.name !== null) { return value.name; }
  return "null";
}
function main(): void { read(undefined); }
