// @target: es2015
// @lib: es5
let id: number = 10000;
/*pruned*/;                  

let person: { b: string; id: number } = { name, id };  // error
function bar(name: string, id: number): { name: number, id: string } { return { name, id }; }  // error
function foo(name: string, id: number): { name: string, id: number } { return { name, id }; }  // error
/*pruned*/;                                                    // ok
let person2: { name: string, id: number } = bar("hello", 5);


function main(): void {}
