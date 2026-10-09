// @target: es2015
let a: number = null;
let b: boolean = null;
let c: string = null;
let d: void = null;

let e: typeof undefined = null;
e = null; // ok

enum E { A }
E.A = null; // error

class C { foo: string }
/*pruned*/;                       
/**/;     // ok
C = null; // error

interface I { foo: string }
let g: I = null as unknown as (I);
g = null; // ok
I = null; // error

/*pruned*/;                      
/**/;     // error

let h: { f(): void } = null;

function i<T>(a: T): void {
    a = null;
}
i = null; // error

function main(): void {}
