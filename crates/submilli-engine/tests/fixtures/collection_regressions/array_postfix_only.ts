// The same metadata requirement applies to ordinary arrays.
function main(): void {
  const values = [1];
  const old = values[0]++;
  assert(old === 1, "postfix-only array program");
}
