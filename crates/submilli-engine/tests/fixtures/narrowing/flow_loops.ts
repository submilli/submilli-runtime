function whileGuard(): string {
 let x: string | null = "ab"; let out = ""; let i = 0;
 if(x !== null) { while(i < 3) { out = out + x; x = "de"; i = i + 1; }  return out + x; }
 return "N";
}
function forGuard(): string {
 let x: string | null = "ab"; let out = ""; let i = 0;
 if(x !== null) { for(let i = 0; i < 3; i = i + 1) { out = out + x; x = "de";  }  return out + x; }
 return "N";
}
function forofGuard(): string {
 let x: string | null = "ab"; let out = ""; let i = 0;
 if(x !== null) { for(const i of [0, 1, 2]) { out = out + x; x = "de";  }  return out + x; }
 return "N";
}
function doGuard(): string {
 let x: string | null = "ab"; let out = ""; let i = 0;
 if(x !== null) { do { out = out + x; x = "de"; i = i + 1; } while(i < 3); return out + x; }
 return "N";
}
interface Node { v: number; next: Node | null; }
function walk(head: Node | null): number {
 let cur = head; let sum = 0; let count = 0;
 if(cur !== null) { while(cur !== null) { sum = sum + cur.v; cur = cur.next; count += 1; if(count > 4) { break; } } }
 return sum;
}
class Cell { value: string | null = "ab"; }
function field(): string { const o = new Cell(); let s = ""; let i = 0;
 if(o.value !== null) { while(i < 3) { s += o.value; o.value = "de"; i += 1; } } return s;
}
function update(): string { let x: string | null = "a"; let s = "";
 if(x !== null) { for(let i = 0; i < 3; x = "b") { s += x; i += 1; } } return s;
}
function main(): void {
 assert(whileGuard() === "abdedede", "while reads writes and joins exit");
 assert(forGuard() === "abdedede", "for reads writes and joins exit");
 assert(forofGuard() === "abdedede", "forof reads writes and joins exit");
 assert(doGuard() === "abdedede", "do reads writes and joins exit");
 assert(walk({v: 1, next: {v: 2, next: {v: 3, next: null}}}) === 6, "guarded linked list terminates");
 assert(walk(null) === 0, "empty list");
 assert(field() === "abdede", "field reload");
 assert(update() === "abb", "update clause reload");
}
