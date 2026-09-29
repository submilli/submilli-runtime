// @target: es2015
// these operators require their operands to be of type Any, the Number primitive type, or
// an enum type
enum E { a, b, c }

/*pruned*/;                           
let b: boolean = null as unknown as (boolean);
let c: number = null as unknown as (number);
let d: string = null as unknown as (string);
let e: { a: number } = null as unknown as ({ a: number });
/*pruned*/;                                 

// All of the below should be an error unless otherwise noted
// operator **
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;       
let r1b2 = b ** b;
let r1b3 = b ** c;
let r1b4 = b ** d;
let r1b5 = b ** e;
/*pruned*/;       

/*pruned*/;        //ok
let r1c2 = c ** b;
let r1c3 = c ** c; //ok
let r1c4 = c ** d;
let r1c5 = c ** e;
/*pruned*/;       

/*pruned*/;       
let r1d2 = d ** b;
let r1d3 = d ** c;
let r1d4 = d ** d;
let r1d5 = d ** e;
/*pruned*/;       

/*pruned*/;       
let r1e2 = e ** b;
let r1e3 = e ** c;
let r1e4 = e ** d;
let r1e5 = e ** e;
/*pruned*/;       

/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;          //ok
let r1g2 = E.a ** b;
let r1g3 = E.a ** c; //ok
let r1g4 = E.a ** d;
let r1g5 = E.a ** e;
/*pruned*/;         

/*pruned*/;          //ok
let r1h2 = b ** E.b;
let r1h3 = c ** E.b; //ok
let r1h4 = d ** E.b;
let r1h5 = e ** E.b;
/*pruned*/;        

function main(): void {}
