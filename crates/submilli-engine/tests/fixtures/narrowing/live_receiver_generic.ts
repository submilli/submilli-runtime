let current: string | string[] = ["old"];
function change(): boolean { current = "x"; return false; }
function main(): void {
  if (!Array.isArray(current) || change()) return;
  assert(current.includes("x"));
  assert(current?.includes("x") === true);
}
