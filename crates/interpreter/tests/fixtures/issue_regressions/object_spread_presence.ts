function source(): { a?: number | null } { return { a: 1 }; }
function main(): void {
  const merged = { a: 0, ...source() };
  merged.a = null;
  assert(JSON.stringify(merged) === '{"a":null}', "required result retains null");
  assert(JSON.stringify({ ...merged }) === '{"a":null}', "required null survives another spread");
  const empty: { a?: number } = {};
  const copied = { ...empty };
  copied.a = 5;
  assert(copied.a === 5, "missing optional retains a writable slot");
}
