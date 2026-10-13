// @target: es2015
enum E { a, b, c }

/*pruned*/;                           
let b: number = null as unknown as (number);
let c: E = null as unknown as (E);

/*pruned*/;                            
/**/;    
/**/;    
/**/;    
/*pruned*/; 
/*pruned*/;      

let x2: number = null as unknown as (number);
/**/;    
x2 **= b;
x2 **= c;
x2 **= null;
x2 **= undefined;

let x3: E = null as unknown as (E);
/**/;    
x3 **= b;
x3 **= c;
x3 **= null;
x3 **= undefined;

function main(): void {}
