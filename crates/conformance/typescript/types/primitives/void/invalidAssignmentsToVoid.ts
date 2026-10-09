// @target: es2015
let x: void = null as unknown as (void);
x = 1;
x = true;
x = '';
x = {}

/*pruned*/;              
/*pruned*/;                       
/**/; 
/**/; 

interface I { foo: string; }
let i: I = null as unknown as (I);
x = i;

/*pruned*/;                      
/**/; 

function f<T>(a: T): void {
    x = a;
}
/**/; 

function main(): void {}
