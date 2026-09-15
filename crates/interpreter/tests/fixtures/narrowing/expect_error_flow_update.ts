// expect-error: the receiver can be `null`
function main(): void { let x: string | null = "a";
 if(x !== null) { for(let i = 0; i < 3; x = null) { const n = x.length; i += 1; } } }
