// expect-error: null
function read(value: string | null | undefined): string {
  if (value !== undefined) { return value; }
  return "missing";
}
function main(): void { read(null); }
