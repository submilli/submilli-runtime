// @target: es2015
let x: typeof undefined = null as unknown as (typeof undefined);

x = 1;
x = '';
x = true;
let a: void = null as unknown as (void);
x = a;
x = null;

class C { foo: string }
/*pruned*/;                       
x = C;
/**/; 

interface I { foo: string }
let c: I = null as unknown as (I);
x = c;

/*pruned*/;                      
/**/; 

x = { f() { } }

function f<T>(a: T): void {
    x = a;
}
/**/; 

enum E { A }
x = E;
x = E.A;

function main(): void {}
