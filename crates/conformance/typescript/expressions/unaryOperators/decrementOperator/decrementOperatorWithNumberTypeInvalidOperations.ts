// @target: es2015
// -- operator on number type
let NUMBER: number = null as unknown as (number);
let NUMBER1: number[] = [1, 2];

function foo(): number { return 1; }

class A {
    public a: number;
    static foo(): number { return 1; }
}
/*pruned*/;  
                                                
 

let objA = new A();

//number type var
/*pruned*/;                     
let ResultIsNumber2 = NUMBER1--;

// number type literal
/*pruned*/;               
/*pruned*/;                           
/*pruned*/;                                                       

let ResultIsNumber6 = 1--;
let ResultIsNumber7 = { x: 1, y: 2 }--;
let ResultIsNumber8 = { x: 1, y: (n: number) => { return n; } }--;

// number type expressions
/*pruned*/;                   
/*pruned*/;                      
/*pruned*/;                                

let ResultIsNumber12 = foo()--;
let ResultIsNumber13 = A.foo()--;
let ResultIsNumber14 = (NUMBER + NUMBER)--;

// miss assignment operator
;   
/**/;     
/**/;   

1--;
NUMBER1--;
foo()--;

function main(): void {}
