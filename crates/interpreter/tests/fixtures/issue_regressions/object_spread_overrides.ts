function main(): void {
  const custom = { toString: (): string => "custom", toJson: (): string => '"json"' };
  const copied = { ...custom };
  assert(copied.toString() === "custom", "preserve toString override");
  assert(JSON.stringify(copied) === '"json"', "preserve Submilli toJson override");
  const nullable: { a: string | null } = { a: null };
  const merged = { a: "old", ...nullable };
  assert(merged.a === null && JSON.stringify(merged) === '{"a":null}', "present null replaces value");
  const contextual: { a?: number | null } = { ...({ a: 1 }) };
  contextual.a = null;
  assert(JSON.stringify(contextual) === '{}', "contextual optional-null model");
}
