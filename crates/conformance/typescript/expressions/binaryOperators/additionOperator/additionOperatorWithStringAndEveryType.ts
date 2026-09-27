// @target: es2015
enum E { a, b, c }

/*pruned*/;                           
let b: boolean = null as unknown as (boolean);
let c: number = null as unknown as (number);
let d: string = null as unknown as (string);
/*pruned*/;                                 
/*pruned*/;                             
/*pruned*/;                       

let x: string = null as unknown as (string);

// string could plus every type, and the result is always string
// string as left operand
/*pruned*/;    
let r2 = x + b;
let r3 = x + c;
let r4 = x + d;
/*pruned*/;    
/*pruned*/;    
/*pruned*/;    

// string as right operand
/*pruned*/;    
let r9 = b + x;
let r10 = c + x;
let r11 = d + x;
/*pruned*/;     
/*pruned*/;     
/*pruned*/;     

// other cases
/*pruned*/;     
let r16 = x + E.a;
let r17 = x + '';
let r18 = x + 0;
let r19 = x + { a: '' };
let r20 = x + [];

function main(): void {}
