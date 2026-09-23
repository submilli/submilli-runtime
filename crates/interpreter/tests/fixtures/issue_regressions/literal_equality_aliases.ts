// Submilli deliberately treats == and != as strict equality aliases.
function read(x: unknown): string {
  if (x == "a") { return x; }
  if (x != "b") { return "other"; }
  return x;
}
function main(): void {
  assert(read("a") === "a" && read("b") === "b" && read(0) === "other", "equality aliases");
}
