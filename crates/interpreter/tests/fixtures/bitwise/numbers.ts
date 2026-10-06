function main(): void {
  assert((13 & 7) === 5, "and");
  assert((8 | 3) === 11, "or");
  assert((13 ^ 7) === 10, "xor");
  assert(~0 === -1, "not");
  assert((1 << 31) === -2147483648, "signed shift");
  assert((-8 >> 2) === -2, "sign extension");
  assert((-1 >>> 0) === 4294967295, "unsigned result");
  assert((4294967297 | 0) === 1, "wrap");
  assert((-4294967297 | 0) === -1, "negative wrap");
  assert((3.9 & 3) === 3 && (-3.9 | 0) === -3, "truncate");
  assert((NaN | 0) === 0 && (Infinity | 0) === 0, "nonfinite");
  assert((-0 | 0) === 0 && (2 ** 100 | 0) === 0, "large and zero");
  assert((1 << 33) === 2 && (1 << -1) === -2147483648, "masked count");
  assert((16 >>> 2.9) === 4, "fractional shift count");
  assert((1 | 2 ^ 3 & 1) === 3, "bitwise precedence");
  assert((1 << 2 + 1) === 8, "shift precedence");
  const continued = 7
    & 3
    ^ 1;
  assert(continued === 2, "newlines");
  let shifted = 1 << 2
  const compared = shifted > 2
  shifted >>= 1
  assert(compared && shifted === 2, "shift does not leave an open cast");
  const complemented = ~
    1;
  assert(complemented === -2, "prefix newline");
  const bytes = new Uint8Array([0, 254, 255]);
  const mapped = bytes.slice(0, 8).map((b: number) => (b + 1) & 0xff);
  assert(mapped[0] === 1 && mapped[1] === 255 && mapped[2] === 0, "SUB-1429");
}
