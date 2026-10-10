let current: string | number = "old";
function change(): boolean { current = 42; return false; }
function main(): void { if (typeof current !== "string" || change()) return; assert(current.toString() === "42"); }
