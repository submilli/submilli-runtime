class Searchable { includes(search: string): boolean { return search === "needle"; } }
let value: string | Searchable = "x";
function change(): boolean { value = new Searchable(); return false; }
function read(): boolean {
  if (typeof value !== "string" || change()) return false;
  return value.includes("needle");
}
function main(): void { assert(read()); }
