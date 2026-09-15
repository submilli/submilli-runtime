function run(): string {
 let x: string | null = "ab";
 let out = ""; let i = 0;
 if (x !== null) { while (i < 3) { out = out + x; x = "de"; i = i + 1; } }
 return out;
}

function main(): void { assert(run() === "abdede", "SUB-902"); }
