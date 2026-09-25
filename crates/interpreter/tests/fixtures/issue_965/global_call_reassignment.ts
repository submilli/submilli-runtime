type Q = { a: number; b: number[] };
type P = { a: number; b: number[]; extra: string };
function mk(): P { return { a: 1, b: [2], extra: "e" }; }
let g: Q | string[] = ["s"];
function reset(): void { g = { a: 9, b: [3] }; }
function main(): void { g = mk(); reset(); assert(g.a === 9, 'read live reassigned object'); assert(g.b.length === 1, 'read live array'); }
