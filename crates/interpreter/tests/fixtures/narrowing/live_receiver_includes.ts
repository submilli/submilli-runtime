let current: string | string[] = "old";
function change(): boolean { current = ["x"]; return false; }
function main(): void { if (typeof current !== "string" || change()) return; assert(current.includes("x")); }
