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
  wide[0] = undefined;
  rejected(captures);
  const empty: unknown[] = [];
  rejected(empty);
  const missing: unknown[] = [undefined];
  rejected(missing);
  const truncated = /a/.exec("a")!;
  truncated.pop();
  rejected(truncated);
  const named = /(?<letter>a)/.exec("a")!;
  const groups: Record<string, unknown> = named.groups!;
  groups["letter"] = 123;
  rejected(named);
  const valid: unknown = /(?<letter>a)/.exec("a");
  const restored = valid as RegExpExecArray;
  assert(restored[0] === "a" && restored.groups!["letter"] === "a");
  const plain: unknown = ["a", undefined];
  const plainMatch = plain as RegExpMatchArray;
  const whole: string = plainMatch[0];
  assert(whole === "a" && plainMatch[1] === undefined);
}
