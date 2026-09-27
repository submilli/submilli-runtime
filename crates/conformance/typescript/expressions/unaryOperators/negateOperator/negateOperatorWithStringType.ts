// @target: es2015
// - operator on string type
let STRING: string = null as unknown as (string);
let STRING1: string[] = ["", "abc"];

function foo(): string { return "abc"; }

/**/;    
                      
                                       
 
/*pruned*/;  
                              
 

/*pruned*/;        

// string type var
let ResultIsNumber1 = -STRING;
let ResultIsNumber2 = -STRING1;

// string type literal
let ResultIsNumber3 = -"";
let ResultIsNumber4 = -{ x: "", y: "" };
let ResultIsNumber5 = -{ x: "", y: (s: string) => { return s; } };

// string type expressions
/*pruned*/;                   
/*pruned*/;                
let ResultIsNumber8 = -STRING1[0];
let ResultIsNumber9 = -foo();
/*pruned*/;                     
let ResultIsNumber11 = -(STRING + STRING);
let ResultIsNumber12 = -STRING.charAt(0);

// miss assignment operators
-"";
-STRING;
-STRING1;
-foo();
/*pruned*/; 

function main(): void {}
