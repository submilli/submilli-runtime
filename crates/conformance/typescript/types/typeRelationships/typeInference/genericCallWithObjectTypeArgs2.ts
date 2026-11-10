// @target: es2015
class Base {
    x: string;
}
class Derived extends Base {
    y: string;
}
class Derived2 extends Base {
    z: string;
}

// returns {}[]
function f<T extends Base, U extends Base>(a: { x: T; y: U }): (T | U)[] {
    return [a.x, a.y];
}

let r = f({ x: new Derived(), y: new Derived2() }); // {}[]
let r2 = f({ x: new Base(), y: new Derived2() }); // {}[]


function f2<T extends Base, U extends Base>(a: { x: T; y: U }): (x: T) => U {
    return (x: T) => a.y;
}

let r3 = f2({ x: new Derived(), y: new Derived2() }); // Derived => Derived2

interface I<T, U> {
    x: T;
    y: U;
}

/*pruned*/;                                                     
/*pruned*/;     // Base => Derived

function main(): void {}
