// @target: es2015
// -- operator on string type
let STRING: string = null as unknown as (string);
let STRING1: string[] = ["", ""];

function foo(): string { return ""; }

class A {
    public a: string;
    static foo(): string { return ""; }
}
/*pruned*/;  
                                                
 

let objA = new A();

// string type var
/*pruned*/;                    
/*pruned*/;                     

let ResultIsNumber3 = STRING--;
let ResultIsNumber4 = STRING1--;

// string type literal
/*pruned*/;                
/*pruned*/;                              
/*pruned*/;                                                        

let ResultIsNumber8 = ""--;
let ResultIsNumber9 = { x: "", y: "" }--;
let ResultIsNumber10 = { x: "", y: (s: string) => { return s; } }--;

// string type expressions
/*pruned*/;                     
/*pruned*/;                  
/*pruned*/;                         
/*pruned*/;                    
/*pruned*/;                      
/*pruned*/;                                

let ResultIsNumber17 = objA.a--;
/*pruned*/;                  
let ResultIsNumber19 = STRING1[0]--;
let ResultIsNumber20 = foo()--;
let ResultIsNumber21 = A.foo()--;
let ResultIsNumber22 = (STRING + STRING)--;

// miss assignment operators
/**/;
/**/;    
/**/;     
/*pruned*/;  
/**/;   
/**/;    
/**/; 
/*pruned*/;   

""--;
STRING--;
STRING1--;
STRING1[0]--;
foo()--;
objA.a--;
/**/; 
/*pruned*/;     

function main(): void {}
