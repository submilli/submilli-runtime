// @target: es2015
// ! operator on string type
let STRING: string = null as unknown as (string);
let STRING1: string[] = ["", "abc"];

function foo(): string { return "abc"; }

/**/;    
                      
                                       
 
/*pruned*/;  
                                                
 

/*pruned*/;        

// string type var
let ResultIsBoolean1 = !STRING;
let ResultIsBoolean2 = !STRING1;

// string type literal
let ResultIsBoolean3 = !"";
let ResultIsBoolean4 = !{ x: "", y: "" };
let ResultIsBoolean5 = !{ x: "", y: (s: string) => { return s; } };

// string type expressions
/*pruned*/;                    
/*pruned*/;                 
let ResultIsBoolean8 = !STRING1[0];
let ResultIsBoolean9 = !foo();
/*pruned*/;                      
let ResultIsBoolean11 = !(STRING + STRING);
let ResultIsBoolean12 = !STRING.charAt(0);

// multiple ! operator
let ResultIsBoolean13 = !!STRING;
let ResultIsBoolean14 = !!!(STRING + STRING);

// miss assignment operators
!"";
!STRING;
!STRING1;
!foo();
/*pruned*/; 

function main(): void {}
