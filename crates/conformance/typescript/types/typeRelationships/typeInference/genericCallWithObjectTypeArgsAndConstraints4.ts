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
/*pruned*/;       
/*pruned*/;         // error because C does not extend D
/*pruned*/;                        
let r4 = foo(null, null);
let r5 = foo({}, null);
let r6 = foo(null, {});
let r7 = foo({}, {});
let r8 = foo(() => { }, () => { });
let r9 = foo(() => { }, () => 1);

function other<T, U extends T>(): void {
    /*pruned*/;        
    /*pruned*/;               // error
}



function main(): void {}
