// @target: es2015
// automatic constructors with a class hieararchy of depth > 2

class Base {
    a: number = 1;
    constructor(x: number) { this.a = x; }
}

class Derived extends Base {
    b: string = '';
    constructor(y: string, z: string) {
        super(2);
        this.b = y;
    }
}

class Derived2 extends Derived {
    x: number = 1
    y: string = 'hello';
}

let r = new Derived(); // error
let r2 = new Derived2(1); // error
let r3 = new Derived('', '');

class Base2<T> {
    a: T;
    constructor(x: T) { this.a = x; }
}

class D<T> extends Base {
    b: T = null;
    constructor(y: T, z: T) {
        super(2);
        this.b = y;
    }
}


/*pruned*/;                            
                 
                
 

/*pruned*/;       // error
/*pruned*/;                  // error
/*pruned*/;                              // ok

function main(): void {}
