let needle: string | null = "x";
function clear(): boolean { needle = null; return false; }
function check(): boolean {
 if (needle !== null && !clear()) return "null".includes(needle);
 return false;
}
function main(): void { assert(check()); }
