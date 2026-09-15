// expect-error: the receiver can be `null`
function main(): void { let x: string | null = null; x = "a"; let i = 0;
 while(i < 3) { const n = x.length; switch(i) { case 0: x = null; break; default: x = "b"; break; } i += 1; } }
