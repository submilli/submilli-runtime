// @target: es2015
// @strict: true
// The && operator permits the operands to be of any type and produces a result of the same
// type as the second operand.

enum E { a, b, c }

/*pruned*/;                            
let a2: boolean = null as unknown as (boolean);
let a3: number = null as unknown as (number);
let a4: string = null as unknown as (string);
/*pruned*/;                              
/*pruned*/;                        
let a7: {} = null as unknown as ({});
let a8: string[] = null as unknown as (string[]);

/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;          
/*pruned*/;           

/*pruned*/;        
let rb2 = a2 && a2;
let rb3 = a3 && a2;
let rb4 = a4 && a2;
/*pruned*/;        
/*pruned*/;        
let rb7 = a7 && a2;
let rb8 = a8 && a2;
let rb9 = null && a2;
let rb10 = null && a2;

/*pruned*/;        
let rc2 = a2 && a3;
let rc3 = a3 && a3;
let rc4 = a4 && a3;
/*pruned*/;        
/*pruned*/;        
let rc7 = a7 && a3;
let rc8 = a8 && a3;
let rc9 = null && a3;
let rc10 = null && a3;

/*pruned*/;        
let rd2 = a2 && a4;
let rd3 = a3 && a4;
let rd4 = a4 && a4;
/*pruned*/;        
/*pruned*/;        
let rd7 = a7 && a4;
let rd8 = a8 && a4;
let rd9 = null && a4;
let rd10 = null && a4;

/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;          
/*pruned*/;           

/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;          
/*pruned*/;           

/*pruned*/;        
let rg2 = a2 && a7;
let rg3 = a3 && a7;
let rg4 = a4 && a7;
/*pruned*/;        
/*pruned*/;        
let rg7 = a7 && a7;
let rg8 = a8 && a7;
let rg9 = null && a7;
let rg10 = null && a7;

/*pruned*/;        
let rh2 = a2 && a8;
let rh3 = a3 && a8;
let rh4 = a4 && a8;
/*pruned*/;        
/*pruned*/;        
let rh7 = a7 && a8;
let rh8 = a8 && a8;
let rh9 = null && a8;
let rh10 = null && a8;

/*pruned*/;          
let ri2 = a2 && null;
let ri3 = a3 && null;
let ri4 = a4 && null;
/*pruned*/;          
/*pruned*/;          
let ri7 = a7 && null;
let ri8 = a8 && null;
let ri9 = null && null;
let ri10 = null && null;

/*pruned*/;          
let rj2 = a2 && null;
let rj3 = a3 && null;
let rj4 = a4 && null;
/*pruned*/;          
/*pruned*/;          
let rj7 = a7 && null;
let rj8 = a8 && null;
let rj9 = null && null;
let rj10 = null && null;

function main(): void {}
