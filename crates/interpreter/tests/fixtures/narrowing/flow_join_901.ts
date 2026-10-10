function sv(s: string | null): string { return s ?? "N"; }
function run(): string {
 let x: string | null = null; x = "A"; let out = ""; let i = 0;
 while (i < 3) { out = out + sv(x); switch(i) { case 1: x = null; break; default: x = "b"; break; } i = i + 1; }
 return out;
}

function main(): void { assert(run() === "AbN", "SUB-901"); }
