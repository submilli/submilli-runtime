// `name` and `message` are plain mutable fields (JS parity) — writable after
// construction, not just inside constructors.
function main(): void {
  const e = new Error("first");
  e.message = "second";
  e.name = "Renamed";
  assert(e.message === "second", "message writable after construction");
  assert(e.name === "Renamed", "name writable after construction");
}
