// @target: es2015
let x: void = null as unknown as (void);

let a: boolean = x;
let b: string = x;
let c: number = x;
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
x = E;
x = E.A;

x = { f() { } }

function main(): void {}
