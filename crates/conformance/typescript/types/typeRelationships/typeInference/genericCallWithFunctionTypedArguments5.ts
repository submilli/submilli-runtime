// @target: es2015
// Generic call with parameter of object type with member of function type of n args passed object whose associated member is call signature with n+1 args

function foo<T, U>(arg: { cb: (t: T) => U }): U {
    return arg.cb(null);
}

/*pruned*/;                       
/*pruned*/;       // {}
// more args not allowed
/*pruned*/;                                  // error
let r3 = foo({ cb: (x: string, y: number) => '' }); // error

function foo2<T, U>(arg: { cb: (t: T, t2: T) => U }): U {
    return arg.cb(null, null);
}

// fewer args ok
/*pruned*/;        // {}
/*pruned*/;                            // {}
let r6 = foo({ cb: (x: string) => '' }); // string
let r7 = foo({ cb: () => '' }); // string


function main(): void {}
