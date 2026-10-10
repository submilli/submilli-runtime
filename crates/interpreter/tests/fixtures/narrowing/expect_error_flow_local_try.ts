// expect-error: the receiver can be `null`
function main(): void { let x: string | null = null; x = "a"; let i = 0;
 while(i < 3) { const n = x.length; try { x = null; } catch(e) { x = "b"; } i += 1; } }
