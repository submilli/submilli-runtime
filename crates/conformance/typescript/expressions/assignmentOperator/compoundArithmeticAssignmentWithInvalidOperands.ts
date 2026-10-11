// @target: es2015
enum E { a, b }

/*pruned*/;                           
let b: void = null as unknown as (void);

let x1: boolean = null as unknown as (boolean);
/**/;   
x1 *= b;
x1 *= true;
x1 *= 0;
x1 *= ''
x1 *= E.a;
x1 *= {};
x1 *= null;
x1 *= undefined;

let x2: string = null as unknown as (string);
/**/;   
x2 *= b;
x2 *= true;
x2 *= 0;
x2 *= ''
x2 *= E.a;
x2 *= {};
x2 *= null;
x2 *= undefined;

let x3: {} = null as unknown as ({});
/**/;   
x3 *= b;
x3 *= true;
x3 *= 0;
x3 *= ''
x3 *= E.a;
x3 *= {};
x3 *= null;
x3 *= undefined;

let x4: void = null as unknown as (void);
/**/;   
x4 *= b;
x4 *= true;
x4 *= 0;
x4 *= ''
x4 *= E.a;
x4 *= {};
x4 *= null;
x4 *= undefined;

let x5: number = null as unknown as (number);
x5 *= b;
x5 *= true;
x5 *= ''
x5 *= {};

/*pruned*/;                        
/**/;   
/*pruned*/;
/**/;   
/**/;    

function main(): void {}
