// @target: es2015
// Generic functions used as arguments for function typed parameters are not used to make inferences from
// Using function arguments, no errors expected

function foo<T>(x: (a: T) => T): T {
    return x(null);
}

/*pruned*/;                   // {}
/*pruned*/;                            // string 
let r3 = foo(x => ''); // {}

function foo2<T, U>(x: T, cb: (a: T) => U): U {
    return cb(x);
}

/*pruned*/;                                         // string, contextual signature instantiation is applied to generic functions
let r5 = foo2(1, (a) => ''); // string
/*pruned*/;                                       

function foo3<T, U>(x: T, cb: (a: T) => U, y: U): U {
    return cb(x);
}

/*pruned*/;                            // string

let r8 = foo3(1, function (a) { return '' }, 1); // error
let r9 = foo3<number, string>(1, (a) => '', ''); // string

function other<T, U>(t: T, u: U): void {
    let r10 = foo2(1, (x: T) => ''); // error
    let r10_2 = foo2(1, (x) => ''); // string

    let r11 = foo3(1, (x: T) => '', ''); // error
    let r11b = foo3(1, (x: T) => '', 1); // error
    let r12 = foo3(1, function (a) { return '' }, 1); // error
}

function main(): void {}
