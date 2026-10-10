let events = "";
class Search { toString(): string { events += "s"; return "x"; } }
class Position { valueOf(): number { events += "p"; return 0; } }
let search: string | Search = "x";
let position: number | Position = 0;
function change(): boolean { search = new Search(); position = new Position(); return false; }
function main(): void {
 if (typeof search !== "string" || typeof position !== "number" || change()) return;
 assert("xyz".includes(search, position));
 assert(events === "sp");
}
