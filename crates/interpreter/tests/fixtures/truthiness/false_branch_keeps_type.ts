function classify(s: string | null): number {
  if (s) {
    return s.length;
  } else {
    if (s === null) {
      return -1;
    }
    return 0;
  }
}

function main(): void {
  assert(classify(null) === -1, "false branch still distinguishes null");
  assert(classify("") === 0, "false branch keeps string | null — \"\" reaches it");
  assert(classify("abc") === 3, "true branch narrows to string");
}
