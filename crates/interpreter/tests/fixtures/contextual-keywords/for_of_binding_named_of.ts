function main(): void {
  let acc = "";
  for (const of of ["a", "b"]) {
    acc += of;
  }
  assert(acc === "ab");

  let total = 0;
  const nums = [1, 2, 3];
  for (const n of nums) {
    total += n;
  }
  assert(total === 6);
}
