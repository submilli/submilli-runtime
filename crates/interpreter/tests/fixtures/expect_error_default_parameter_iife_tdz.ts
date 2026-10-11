// expect-error: cannot access `later` before its initialization
function read(value: number = (() => later)(), later: number = 5): number {
  return value;
}

function main(): void {
  read();
}
