// @target: es2015
let x: void = null as unknown as (void);
x = 1;
x = '';
x = true;

enum E { A }
x = E;
x = E.A;

/*pruned*/;             
/*pruned*/;                       
/**/; 

interface I { foo: string }
let b: I = null as unknown as (I);
x = b;

x = { f() {} }

/*pruned*/;                      
/**/; 

function f<T>(a: T): void {
    x = a;
}
/**/; 

function main(): void {}
