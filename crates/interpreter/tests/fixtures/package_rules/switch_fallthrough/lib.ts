// expect-error: no fallthrough
// expect-error-count: 1
function pick(value: number): void {
  switch (value) {
    case 1: {
      const seen = value;
    }
    case 2:
      break;
  }
}
