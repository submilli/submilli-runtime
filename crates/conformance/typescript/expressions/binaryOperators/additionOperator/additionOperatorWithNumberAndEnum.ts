// @target: es2015
enum E { a, b }
enum F { c, d }

let a: number = null as unknown as (number);
/*pruned*/;                       
/*pruned*/;                               

let r1 = a + a;
/*pruned*/;    
/*pruned*/;    
/*pruned*/;    

let r5 = 0 + a;
let r6 = E.a + 0;
let r7 = E.a + E.b;
let r8 = E['a'] + E['b'];
let r9 = E['a'] + F['c'];

/*pruned*/;     
/*pruned*/;     
/*pruned*/;     
/*pruned*/;     
/*pruned*/;     


function main(): void {}
