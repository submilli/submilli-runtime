// @target: es2015
let x: typeof undefined = null as unknown as (typeof undefined);

let a: number = x;
let b: boolean = x;
let c: string = x;
let d: void = x;

let e: typeof undefined = x;
e = x; // should work

class C { foo: string }
/*pruned*/;                       
/**/; 

interface I { foo: string }
let g: I = null as unknown as (I);
g = x;

let h: { f(): void } = x;

function i<T>(a: T): void {
    a = x;
}

function main(): void {}
