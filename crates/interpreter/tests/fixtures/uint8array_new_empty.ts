// SUB-1758: `new Uint8Array()` with no argument is empty, as in tsc and JavaScript.
function main(): void {
 const u = new Uint8Array();
 assert(u.length === 0);
 assert(new Uint8Array(2).length === 2 && new Uint8Array([1]).length === 1);
}
