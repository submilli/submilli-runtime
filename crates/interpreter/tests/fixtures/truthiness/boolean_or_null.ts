function pick(b: boolean | null): number {
  if (b) {
    return 1;
  }
  return 0;
}

function main(): void {
  assert(pick(true) === 1, "true is truthy");
  assert(pick(false) === 0, "false is falsy");
  assert(pick(null) === 0, "null is falsy");
}
