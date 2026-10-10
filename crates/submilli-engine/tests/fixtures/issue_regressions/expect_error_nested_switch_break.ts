// expect-error: no fallthrough
function main(): void { switch (1) { case 1: switch (2) { case 2: break; default: break; } default: break; } }
