// @target: es2015
let x: typeof undefined = null as unknown as (typeof undefined);

enum E { A }
E = x;
E.A = x;

class C { foo: string }
/*pruned*/;                       
C = x;

interface I { foo: string }
let g: I = null as unknown as (I);
g = x;
I = x;

/*pruned*/;                      
/**/; 

function i<T>(a: T): void { }
// BUG 767030
i = x; 

function main(): void {}
