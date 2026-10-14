// Case labels written with bitwise and exponent operators select the value
// they compute.
function label(v: number): string {
  switch (v) {
    case 1 << 2:
      return "shl";
    case -1 >>> 28:
      return "ushr";
    case 2 ** 5:
      return "pow";
    case ~5:
      return "not";
    case 5 ^ 3:
      return "xor";
    case 12 & 10:
      return "and";
    default:
      return "none";
  }
}

function main(): void {
  assert(label(4) === "shl", "1 << 2");
  assert(label(15) === "ushr", "-1 >>> 28");
  assert(label(32) === "pow", "2 ** 5");
  assert(label(-6) === "not", "~5");
  assert(label(6) === "xor", "5 ^ 3");
  assert(label(8) === "and", "12 & 10");
  assert(label(7) === "none", "no label");
}
