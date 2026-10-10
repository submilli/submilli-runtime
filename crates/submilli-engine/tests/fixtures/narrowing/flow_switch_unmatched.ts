function f(k: number): string {
 let x: string | null = "a";
 if (x !== null) {
  switch(k) { case 1: x = "b"; break; }
  return x.toUpperCase();
 }
 return "N";
}
function main(): void { assert(f(0) === "A", "zero"); assert(f(1) === "B", "case"); }
