function main(): number {
  for (let value = 7, read = (): number => value; value < 8; ) {
    value = 8;
    assert(read() === 7);
    break;
  }
  let total = 0;
  for (let value = 0, read = (): number => value; value < 3; value++) {
    assert(read() === 0);
    total += value;
  }
  assert(total === 3);
  return 0;
}
