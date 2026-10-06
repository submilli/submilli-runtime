function main(): void {
  let n = 255;
  n &= 15; n |= 16; n ^= 3; n <<= 2; n >>= 1; n >>>= 1;
  assert(n === 28, "number compounds");
  let b = 255n;
  b &= 15n; b |= 16n; b ^= 3n; b <<= 2n; b >>= 2n;
  assert(b === 28n, "bigint compounds");
  const values = [7];
  let reads = 0;
  function index(): number { reads += 1; return 0; }
  const assigned = (values[index()] &= 3);
  assert(reads === 1 && assigned === 3 && values[0] === 3, "single index evaluation");
  const box = {value: 3};
  box.value <<= 2;
  assert(box.value === 12, "field assignment");
  let left = 1; let right = 7;
  left |= right &= 3;
  assert(left === 3 && right === 3, "right associativity");
}
