// @strict: true
// @target:es5, es2015
let f1 = () => { }
let f2 = (x: string, y: string) => { }
/*pruned*/;                                    
let f4 = (x: string, y: number, z: number = 10) => { }
function foo(func: () => boolean): void { }
foo(() => true);
foo(() => { return false; });

function main(): void {}
