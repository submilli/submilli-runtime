// @target: es2015
// @strict: true
// it is always illegal to provide type arguments to a non-generic function
// all invocations here are illegal

function f(x: number): null { return null; }
let r = f<string>(1);

let f2 = (x: number) => { return null; }
let r2 = f2<string>(1);

/*pruned*/;                                                                
/*pruned*/;            

class C {
    f(x: number): null {
        return null;
    }
}
let r4 = (new C()).f<string>(1);

/*pruned*/;  
                      
 
/*pruned*/;                       
/*pruned*/;             

class C2 {
    f(x: number): null {
        return null;
    }
}
let r6 = (new C2()).f<string>(1);

/*pruned*/;   
                 
 
/*pruned*/;                          
/*pruned*/;              

/**/; 
/*pruned*/;          

/*pruned*/;                            
/*pruned*/;             

function main(): void {}
