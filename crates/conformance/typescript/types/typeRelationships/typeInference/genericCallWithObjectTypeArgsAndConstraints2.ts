// @target: es2015
// Generic call with constraints infering type parameter from object member properties
// No errors expected

class Base {
    x: string;
}
class Derived extends Base {
    y: string;
}

/*pruned*/;                                           
                                      
             
 
/*pruned*/;                                        
/*pruned*/;                                            


interface I<T> {
    a: T;
}
/*pruned*/;                              
                                      
             
 
/*pruned*/;                                         
/*pruned*/;    


function f3<T extends Base>(x: T, y: (a: T) => T): T {
    return y(null);
}
let r4 = f3(new Base(), x => x);
let r5 = f3(new Derived(), x => x);

let r6 = f3(null, null); // any
let r7 = f3(null, x => x); // any


function main(): void {}
