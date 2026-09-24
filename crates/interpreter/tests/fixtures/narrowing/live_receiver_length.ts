let value: string | number[] = "x";
function change(): boolean { value = [1, 2, 3]; return false; }
function read(): number {
  if (typeof value !== "string" || change()) return 0;
  return value.length;
}
function main(): void { assert(read() === 3); }
