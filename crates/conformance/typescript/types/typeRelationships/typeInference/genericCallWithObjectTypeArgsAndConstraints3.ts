// @target: es2015
// Generic call with constraints infering type parameter from object member properties

class Base {
    x: string;
}
class Derived extends Base {
    y: string;
}
class Derived2 extends Base {
    z: string;
}

/*pruned*/;                                       
                                       
             
 

/*pruned*/;                                          // error because neither is supertype of the other

/*pruned*/;                                                     
                                       
             
 

/*pruned*/;                                           // ok
/*pruned*/;                                           // ok


function f3<T extends Base>(y: (a: T) => T, x: T): T {
    return y(null);
}

// all ok - second argument is processed before x is fixed
let r4 = f3(x => x, new Base());
let r5 = f3(x => x, new Derived());
let r6 = f3(x => x, null);


function main(): void {}
