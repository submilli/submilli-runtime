function f(k: number): string { switch(k) { case 1: try { return "one"; } finally { if(k === 1) { return "s1r"; } } default: return "def"; } }
function main(): void { assert(f(1) === "s1r", "finally return"); assert(f(0) === "def", "default"); assert(caught(1) === 2, "catch"); assert(nested(1) === 3, "nested switch"); }

function caught(k: number): number {
 switch(k) { case 1: try { throw new Error("x"); } catch(e) { return 2; } default: return 0; }
}
function nested(k: number): number {
 switch(k) { case 1: switch(k) { case 1: return 3; default: return 4; } default: return 0; }
}
