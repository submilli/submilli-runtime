// @target: es2015
// ++ operator on string type
let STRING: string = null as unknown as (string);
let STRING1: string[] = ["", ""];

function foo(): string { return ""; }

/**/;    
                      
                                       
 
/*pruned*/;  
                                                
 

/*pruned*/;        

// string type var
/*pruned*/;                    
/*pruned*/;                     

let ResultIsNumber3 = STRING++;
let ResultIsNumber4 = STRING1++;

// string type literal
/*pruned*/;                
/*pruned*/;                              
/*pruned*/;                                                        

let ResultIsNumber8 = ""++;
let ResultIsNumber9 = { x: "", y: "" }++;
let ResultIsNumber10 = { x: "", y: (s: string) => { return s; } }++;

// string type expressions
/*pruned*/;                     
/*pruned*/;                  
/*pruned*/;                         
/*pruned*/;                    
/*pruned*/;                      
/*pruned*/;                                

/*pruned*/;                     
/*pruned*/;                  
let ResultIsNumber19 = STRING1[0]++;
let ResultIsNumber20 = foo()++;
/*pruned*/;                      
let ResultIsNumber22 = (STRING + STRING)++;

// miss assignment operators
/**/;
/**/;    
/**/;     
/*pruned*/;  
/**/;   
/**/;    
/**/; 
/*pruned*/;   

""++;
STRING++;
STRING1++;
STRING1[0]++;
foo()++;
/**/;    
/**/; 
/*pruned*/;     

function main(): void {}
