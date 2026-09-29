// @target: es2015
class Base {
    a: number = 1;
    constructor(x: number) { this.a = x; }
}

class Derived extends Base {
    x: number = 1
    y: string = 'hello';
}

let r = new Derived(); // error
let r2 = new Derived(1); 

class Base2<T> {
    a: T;
    constructor(x: T) { this.a = x; }
}

/*pruned*/;                               
                 
                
 

/*pruned*/;      // error
/*pruned*/;                 // ok

function main(): void {}
