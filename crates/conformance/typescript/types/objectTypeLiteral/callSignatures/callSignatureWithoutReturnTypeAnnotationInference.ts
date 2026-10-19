// @target: es2015
// @strict: true
// @allowUnreachableCode: true

// Call signatures without a return type should infer one from the function body (if present)

// Simple types
/*pruned*/;              
             
 
/*pruned*/;    

/*pruned*/;               
                  
 
/*pruned*/;      

function foo3(): never {
    return foo3();
}
let r3 = foo3();

function foo4<T>(x: T): T {
    return x;
}
let r4 = foo4(1);

/*pruned*/;              
               
                 
            
                 
     
 
/*pruned*/;      

/*pruned*/;                
         
     
               
                  
     
             
                  
     
 
/*pruned*/;      

/*pruned*/;                                                                                                    
                    
 
/*pruned*/;      

// object types
function foo8(x: number): { x: number; } {
    return { x: x };
}
let r8 = foo8(1);

interface I {
    foo: string;
}
function foo9(x: number): I {
    let i: I = null as unknown as (I);
    return i;    
}
let r9 = foo9(1);

class C {
    foo: string;
}
/*pruned*/;                   
                                      
             
 
/*pruned*/;        

/*pruned*/;  
                     
                                  
 
/*pruned*/;                 
             
 
/*pruned*/;       

// merged declarations
interface I2 {
    x: number;
}
/*pruned*/;   
              
 
function foo12(): I2 {
    let i2: I2 = null as unknown as (I2);
    return i2;
}
let r12 = foo12();

function m1(): number { return 1; }
/*pruned*/;                       
function foo13(): typeof m1 {
    return m1;
}
let r13 = foo13();

/**/;     
                
                      
 
/*pruned*/;   
                     
 
/*pruned*/;                  
              
 
/*pruned*/;       

enum e1 { A }
/*pruned*/;                       
/*pruned*/;                  
              
 
/*pruned*/;       

function main(): void {}
