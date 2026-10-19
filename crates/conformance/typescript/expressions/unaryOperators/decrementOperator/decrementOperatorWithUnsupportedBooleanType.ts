// @target: es2015
// -- operator on boolean type
let BOOLEAN: boolean = null as unknown as (boolean);

function foo(): boolean { return true; }

class A {
    public a: boolean;
    static foo(): boolean { return true; }
}
/*pruned*/;  
                                                  
 

let objA = new A();

// boolean type var
/*pruned*/;                     

let ResultIsNumber2 = BOOLEAN--;

// boolean type literal
/*pruned*/;                  
/*pruned*/;                                   
/*pruned*/;                                                           

let ResultIsNumber6 = true--;
let ResultIsNumber7 = { x: true, y: false }--;
let ResultIsNumber8 = { x: true, y: (n: boolean) => { return n; } }--;

// boolean type expressions
/*pruned*/;                    
/*pruned*/;                  
/*pruned*/;                    
/*pruned*/;                      

let ResultIsNumber13 = foo()--;
let ResultIsNumber14 = A.foo()--;
let ResultIsNumber15 = objA.a--;
/*pruned*/;                  

// miss assignment operators
/**/;  
/**/;     
/**/;   
/**/;    
/**/; 
/*pruned*/;   

true--;
BOOLEAN--;
foo()--;
objA.a--;
/**/; 
/*pruned*/;     

function main(): void {}
