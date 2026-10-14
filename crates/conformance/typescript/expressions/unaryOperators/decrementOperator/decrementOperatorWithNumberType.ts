// @target: es2015
// -- operator on number type
let NUMBER: number = null as unknown as (number);
let NUMBER1: number[] = [1, 2];

class A {
    public a: number;
}
/*pruned*/;  
                                                
 

let objA = new A();

// number type var
/*pruned*/;                    

let ResultIsNumber2 = NUMBER--;

// expressions
/*pruned*/;                    
/*pruned*/;                 

let ResultIsNumber5 = objA.a--;
/*pruned*/;                 
let ResultIsNumber7 = NUMBER1[0]--;

// miss assignment operators
/**/;    

/*pruned*/;  
/**/;    
/**/; 
/*pruned*/;   

NUMBER--;
NUMBER1[0]--;
objA.a--;
/**/; 
/*pruned*/;     

function main(): void {}
