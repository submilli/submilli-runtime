// @target: es2015
// @lib: es5
let id: number = 10000;
/*pruned*/;                  

/*pruned*/;                                             
function foo( obj:{ name: string }): void { };
function bar(name: string, id: number): { name: string; id: number; } { return { name, id }; }
function bar1(name: string, id: number): { name: string; } { return { name }; }
function baz(name: string, id: number): { name: string; id: number } { return { name, id }; }

/*pruned*/; 
let person1 = bar("Hello", 5);
let person2: { name: string } = bar("Hello", 5);
let person3: { name: string; id:number } = bar("Hello", 5);


function main(): void {}
