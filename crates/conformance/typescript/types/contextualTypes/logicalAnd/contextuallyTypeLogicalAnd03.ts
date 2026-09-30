// @target: es2015
// @noImplicitAny: true

let x: (a: string) => string = null as unknown as ((a: string) => string);
let y = true;

x = (a => a) && (b => b);

function main(): void {}
