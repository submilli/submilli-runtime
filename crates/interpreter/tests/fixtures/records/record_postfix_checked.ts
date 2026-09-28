function bump(values: Record<"x" | "y", number>, key: "x" | "y"): number {
  return values[key]++;
}
function main(): void {
  const values: Record<"x" | "y", number> = { x: 1, y: 2 };
  const writable: Record<string, unknown> = values;
  writable["x"] = "10";
  let caught = false;
  try { const before = bump(values, "x"); }
  catch (e: Error) { caught = e.name === "TypeError"; }
  assert(caught);
  assert(writable["x"] === "10");
}
