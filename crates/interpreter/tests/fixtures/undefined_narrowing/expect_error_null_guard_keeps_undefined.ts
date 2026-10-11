// expect-error: undefined
function read(value: string | null | undefined): string {
  if (value !== null) { return value; }
  return "null";
}
function main(): void { read(undefined); }
