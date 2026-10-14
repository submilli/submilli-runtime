// @target: es2015
// @lib: es5
let id: number = 10000;
/*pruned*/;                  

let person: { b: string; id: number } = { name, id };  // error
/*pruned*/;                                                     // ok
function foo(name: string, id: number): { id: string, name: number } { return { name, id }; }  // error
function bar(obj: { name: string; id: boolean }): void { }
bar({ name, id });  // error



function main(): void {}
