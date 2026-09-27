// @target: es2015
// type parameter lists must exactly match type argument lists
// all of these invocations are errors

function f<T, U>(x: T, y: U): T { return null; }
let r1 = f<number>(1, '');
let r1b = f<number, string, number>(1, '');

/*pruned*/;                                       
/*pruned*/;                
/*pruned*/;                                 

/*pruned*/;                                                                          
/*pruned*/;                
/*pruned*/;                                 

/**/;    
                            
                    
     
 
/*pruned*/;                         
/*pruned*/;                                          

interface I {
    f<T, U>(x: T, y: U): T;
}
/*pruned*/;                       
/*pruned*/;                 
/*pruned*/;                                  

class C2<T, U> {
    f(x: T, y: U): T {
        return null;
    }
}
let r6 = (new C2()).f<number>(1, '');
let r6b = (new C2()).f<number, string, number>(1, '');

interface I2<T, U> {
    f(x: T, y: U): T;
}
/*pruned*/;                                                          
/*pruned*/;                  
/*pruned*/;                                   

function main(): void {}
