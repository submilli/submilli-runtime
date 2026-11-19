function replacement(n: number): number {
  return 99;
}

function main(): number {
  const values = [1, 2];
  values.push = replacement;
  return values.push(3) === 99 && values.length === 2 ? 0 : 1;
}
