class Searchable { includes(search: string): boolean { return search === "needle"; } }
let value: string | Searchable = "x";
function change(): boolean { value = new Searchable(); return false; }
function readCurrent(): string | Searchable { return value; }
function read(): boolean {
  if (typeof value !== "string" || change()) return false;
  const actual = readCurrent();
  if (typeof actual === "string") throw new Error("change must leave a Searchable");
  return actual.includes("needle");
}
function main(): void { assert(read()); }
