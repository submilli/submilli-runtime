// expect-error: no fallthrough
function main(): void { switch (1) { case 1: try { console.log("body"); } finally { console.log("finally"); } default: break; } }
