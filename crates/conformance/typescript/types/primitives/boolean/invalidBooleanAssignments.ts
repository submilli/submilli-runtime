// @target: es2015
let x = true;

let a: number = x;
let b: string = x;
let c: void = x;
let d: typeof undefined = x;

enum E { A }
let e: E = x;

class C { foo: string }
let f: C = x;

interface I { bar: string }
let g: I = x;

/*pruned*/;               
let h2: { toString(): string } = x; // no error

/*pruned*/;                      
/**/; 

function i<T>(a: T): void {
    a = x;
}
i = x;

function main(): void {}
