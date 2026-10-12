// @target: es2015
enum E {
    A,
    B
}

let n: number = null as unknown as (number);
/*pruned*/;                           
let e: E = null as unknown as (E);

n = e;
n = E.A;

/**/; 
/**/; 
/**/;   

e = e;
e = E.A;
e = E.B;
e = n;
e = null;
e = undefined;
e = 1;
e = 1.;
e = 1.0;
e = -1;

function main(): void {}
