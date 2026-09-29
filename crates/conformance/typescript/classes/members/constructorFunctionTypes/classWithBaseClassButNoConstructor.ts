// @target: es2015
class Base {
    constructor(x: number) { }
}

class C extends Base {
    foo: string;
}

/**/;     
let c = new C(); // error
let c2 = new C(1); // ok

class Base2<T,U> {
    constructor(x: T) { }
}

class D<T,U> extends Base2<T,U> {
    foo: U;
}

/*pruned*/;
let d = new D(); // error
let d2 = new D(1); // ok

// specialized base class
class D2<T, U> extends Base2<string, number> {
    foo: U;
}

/*pruned*/; 
let d3 = new D(); // error
let d4 = new D(1); // ok

class D3 extends Base2<string, number> {
    foo: string;
}

/*pruned*/; 
let d5 = new D(); // error
let d6 = new D(1); // ok

function main(): void {}
