function run(): string { let v: number | null = 1; let i = 0; if (v !== null) { while (i < 1) { v = 7; i = i + 1; } return v.toString(); } return "N"; }

function main(): void { assert(run() === "7", "SUB-898"); }
