// @strict: true
// @target: es2015
// When a function expression with no type parameters and no parameter type annotations 
// is contextually typed (section 4.19) by a type T and a contextual signature S can be extracted from T

enum E { red, blue }

// A contextual signature S is extracted from a function type T as follows:
//      If T is a function type with exactly one call signature, and if that call signature is non- generic, S is that signature.

let a0: (n: number, s: string) => number = (num, str) => {
    num.toExponential();
    return 0;
}

class Class<T> {
    foo(): void { }
}

let a1: (c: Class<Number>) => number = (a1) => {
    a1.foo();
    return 1;
}

// A contextual signature S is extracted from a function type T as follows:
//      If T is a union type, let U be the set of element types in T that have call signatures.
//        If each type in U has exactly one call signature and that call signature is non- generic,
//        and if all of the signatures are identical ignoring return types,
//        then S is a signature with the same parameters and a union of the return types.
let b1: ((s: string, w: boolean) => void) | ((s: string, w: boolean) => string) = null as unknown as (((s: string, w: boolean) => void) | ((s: string, w: boolean) => string));
b1 = (k, h) => { };
let b2: typeof a0 | ((n: number, s: string) => string) = null as unknown as (typeof a0 | ((n: number, s: string) => string));
b2 = (foo, bar) => { return foo + 1; }
b2 = (foo, bar) => { return "hello"; }
let b3: (name: string, num: number, boo: boolean) => void = null as unknown as ((name: string, num: number, boo: boolean) => void);
b3 = (name, number) => { };

let b4: (n: E) => string = (number: E = 1) => { return "hello"; };
let b5: (n: {}) => string = (number: {} = "string") => { return "hello"; };

// A contextual signature S is extracted from a function type T as follows:
//      Otherwise, no contextual signature can be extracted from T and S is undefined.
let b6: ((s: string, w: boolean) => void) | ((n: number) => number) = null as unknown as (((s: string, w: boolean) => void) | ((n: number) => number));
let b7: ((s: string, w: boolean) => void) | ((s: string, w: number) => string) = null as unknown as (((s: string, w: boolean) => void) | ((s: string, w: number) => string));
/*pruned*/;                     
/*pruned*/;  
                      
             
                     // Per spec, no contextual signature can be extracted in this case. (Otherwise clause)
/*pruned*/;          // Per spec, no contextual signature can be extracted in this case. (Otherwise clause)

class C<T, U> {
    constructor() {
        /*pruned*/;                                                                    
                          
            // Per spec, no contextual signature can be extracted in this case.
    }
}

function main(): void {}
