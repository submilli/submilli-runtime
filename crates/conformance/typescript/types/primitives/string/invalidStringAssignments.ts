// @target: es2015
let x = '';

let a: boolean = x;
let b: number = x;
let c: void = x;
let d: typeof undefined = x;

class C { foo: string; }
let e: C = x;

interface I { bar: string; }
let f: I = x;

let g: { baz: string } = 1;
/*pruned*/;               

/*pruned*/;                      
/**/; 

function i<T>(a: T): void {
    a = x;
}
i = x;

enum E { A }
let j: E = x;

function main(): void {}
