// @target: es2015
// derived class constructors must contain a super call

class Base {
    x: string;
}

class Derived extends Base {
    constructor() { // error
    }
}

class Base2<T> {
    x: T;
}

class Derived2<T> extends Base2<T> {
    constructor() { // error for no super call (nested scopes don't count)
        let r2 = () => super(); // error for misplaced super call (nested function)
    }
}

class Derived3<T> extends Base2<T> {
    constructor() { // error
        let r = function () { /**/;   } // error
    }
}

/*pruned*/;                         
                   
                              
     
 

function main(): void {}
