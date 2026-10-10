// @target: es2015
enum E { a, b }

let a: void = null as unknown as (void);

let x1: boolean = null as unknown as (boolean);
x1 += a;
x1 += true;
x1 += 0;
x1 += E.a;
x1 += {};
x1 += null;
x1 += undefined;

let x2: {} = null as unknown as ({});
x2 += a;
x2 += true;
x2 += 0;
x2 += E.a;
x2 += {};
x2 += null;
x2 += undefined;

let x3: void = null as unknown as (void);
x3 += a;
x3 += true;
x3 += 0;
x3 += E.a;
x3 += {};
x3 += null;
x3 += undefined;

let x4: number = null as unknown as (number);
x4 += a;
x4 += true;
x4 += {};

/*pruned*/;                        
/**/;   
/*pruned*/;
/**/;    

function main(): void {}
