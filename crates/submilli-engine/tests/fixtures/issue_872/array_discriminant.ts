class Base { v: number[] | string | null = null; }
class Mid extends Base { v: number[] | null = null; }
class Sub extends Mid { v: number[] = []; }
function main(): void {
 const p = new Sub();
 const b: number[] = [];
 for (let i = 0; i < 2000; i++) { b.push(i); }
 p.v = b;
 let s = 0;
 for (let i = 0; i < 100; i++) { s = s + p.v.length; }
 assert(s === 200000, 'three-level array narrowing');
}
