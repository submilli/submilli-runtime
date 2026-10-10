function main(): void {
  const original = { x: 1, hidden: "bad" };
  const visible: { x: number } = original;
  const record: Record<string, number> = visible;
  const key: string = "hidden";
  let caught = false;
  try { const value = record[key]; } catch (e: Error) { caught = true; }
  assert(caught);
  const writable: Record<string, unknown> = record;
  writable["x"] = "changed";
  caught = false;
  try { const value = record.x; } catch (e: Error) { caught = true; }
  assert(caught);
}
