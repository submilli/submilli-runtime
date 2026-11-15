let current: string | string[] = ["old"];
function change(): boolean { current = "x"; return false; }
// The declared return type lets both front ends re-check the post-call value.
function readCurrent(): string | string[] { return current; }
function main(): void {
  if (!Array.isArray(current) || change()) return;
  const actual = readCurrent();
  if (typeof actual !== "string") throw new Error("change must leave a string");
  assert(actual.includes("x"));
  assert(actual?.includes("x") === true);
}
