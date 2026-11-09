// @target: es2015
// Generic call with constraints infering type parameter from object member properties

class C {
    x: string;
}

class D {
    x: string;
    y: string;
}

function foo<T, U extends T>(t: T, t2: U): (x: T) => U {
    return (x: T) => t2;
}

/*pruned*/;                       
/*pruned*/;                       
/*pruned*/;         // the constraints are self-referencing, no downstream error
let r9 = foo(() => 1, () => { }); // the constraints are self-referencing, no downstream error

function other<T, U extends T>(): void {
    /*pruned*/;               // error
}


function main(): void {}
