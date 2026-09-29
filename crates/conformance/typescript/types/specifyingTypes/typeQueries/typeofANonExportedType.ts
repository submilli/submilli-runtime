// @module: commonjs
// @target: es2015
let x = 1;
let r1: typeof x = null as unknown as (typeof x);
let y = { foo: '' };
let r2: typeof y = null as unknown as (typeof y);
class C {
    foo: string;
}
/*pruned*/;                       
/*pruned*/;                        

let r3: typeof C = null as unknown as (typeof C);
/*pruned*/;                                      
/*pruned*/;                                         

interface I {
    foo: string;
}
let i: I = null as unknown as (I);
let i2: I = null as unknown as (I);
let r5: typeof i = null as unknown as (typeof i);
let r5_2: typeof i2 = null as unknown as (typeof i2);

/*pruned*/;  
                        
                    
                    
     
 
/*pruned*/;                                      
/*pruned*/;                                              

/*pruned*/;  
/*pruned*/;                                      
/*pruned*/;                                              

enum E {
    A
}
let r10: typeof E = null as unknown as (typeof E);
let r11: typeof E.A = null as unknown as (typeof E.A);

let r12: typeof r12 = null as unknown as (typeof r12);

function foo(): void { }
/*pruned*/;    
                     
                    
                    
     
 
let r13: typeof foo = null as unknown as (typeof foo);

function main(): void {}
