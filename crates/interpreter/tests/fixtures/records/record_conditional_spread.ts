function choose(flag: boolean): void {
  const a = { x: 3 };
  const result = { ["tag"]: 1, ...(flag ? a : {}) };
  assert(("x" in result) === flag);
  assert(result.x === (flag ? 3 : undefined));
}
function main(): void { choose(true); choose(false); }
