// @target: es2015
// valid invocations of generic functions with no explicit type arguments provided 

function f<T>(x: T): T { return null; }
let r = f(1);

/*pruned*/;                              
/*pruned*/;    

/*pruned*/;                                                        
/*pruned*/;    

/**/;    
                   
                    
     
 
/*pruned*/;             

interface I {
    f<T>(x: T): T;
}
/*pruned*/;                       
/*pruned*/;     

class C2<T> {
    f(x: T): T {
        return null;
    }
}
let r6 = (new C2()).f(1);

interface I2<T> {
    f(x: T): T;
}
/*pruned*/;                                          
/*pruned*/;      

function main(): void {}
