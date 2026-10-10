function run(): string { let s: string | null = null; try { s = "t"; } catch(e) { s = "c"; } return s.toUpperCase(); }

function main(): void { assert(run() === "T", "SUB-820"); }
