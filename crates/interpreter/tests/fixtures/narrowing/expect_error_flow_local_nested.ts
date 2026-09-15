// expect-error: the receiver can be `null`
function main(): void { let x: string | null = null; x = "a"; let i = 0;
 while(i < 3) { const n = x.length; let j = 0; while(j < 1) { x = null; j += 1; } i += 1; } }
