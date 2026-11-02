function rejected(value: unknown): void {
  let failures = 0;
  try { const checked = value as RegExpExecArray; } catch (e: TypeError) { failures++; }
  try { const checked = value as RegExpMatchArray; } catch (e: TypeError) { failures++; }
  assert(failures === 2);
}
function main(): void {
  const captures = /a/.exec("a")!;
  const wide: unknown[] = captures;
  wide[0] = 123;
  rejected(captures);
  const named = /(?<letter>a)/.exec("a")!;
  const groups: Record<string, unknown> = named.groups!;
  groups["letter"] = 123;
  rejected(named);
  const valid: unknown = /(?<letter>a)/.exec("a");
  const restored = valid as RegExpExecArray;
  assert(restored[0] === "a" && restored.groups!["letter"] === "a");
}
