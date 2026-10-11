class Runner { run(): void {} }
function bare(): void {}
function main(): void {
  const runner: Runner | null = new Runner();
  let chainThrew = false;
  try { runner?.run()!; } catch (error: TypeError) { chainThrew = true; }
  assert(chainThrew, "non-null assertion checks an optional void result");
  let bareThrew = false;
  try { bare()!; } catch (error: TypeError) { bareThrew = true; }
  assert(bareThrew, "non-null assertion checks a direct void result");
}
