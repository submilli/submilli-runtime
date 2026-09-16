// expect-error: closure body reassigns `x`
function main(): void { let x: string | null = "a"; const mut = (): void => { x = null; }; if(x !== null) { mut(); console.log(x.length); } }
