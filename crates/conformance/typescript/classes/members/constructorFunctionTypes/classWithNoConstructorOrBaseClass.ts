// @target: es2015
class C {
    x: string;
}

let c = new C();
/**/;     

class D<T,U> {
    x: T;
    y: U;
}

let d = new D();
let d2 = new D<string, number>();
/*pruned*/;


function main(): void {}
