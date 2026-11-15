let current: string | string[] = "old";
function change(): boolean { current = ["x"]; return false; }
// The declared return type lets both front ends re-check the post-call value.
function readCurrent(): string | string[] { return current; }
function main(): void {
  if (typeof current !== "string" || change()) return;
  const actual = readCurrent();
  if (!Array.isArray(actual)) throw new Error("change must leave an array");
  assert(actual?.includes("x") === true);
  assert(actual?.length === 1);
}
