let search: string | RegExp = "x";
function change(): boolean { search = /x/; return false; }
function read(): string {
  search = "x";
  if (typeof search !== "string" || change()) return "";
  return search;
}
function main(): void {
  let count = 0;
  try { "xyz".includes(read()); } catch (error) { count++; }
  try { "xyz".startsWith(read()); } catch (error) { count++; }
  try { "xyz".endsWith(read()); } catch (error) { count++; }
  assert(count === 3);
}
