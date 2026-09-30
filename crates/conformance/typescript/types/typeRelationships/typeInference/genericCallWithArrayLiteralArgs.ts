// @target: es2015
function foo<T>(t: T): T {
    return t;
}

let r = foo([1, 2]); // number[]
let r_2 = foo<number[]>([1, 2]); // number[]
/*pruned*/;                  // any[]
let r2 = foo([]); // any[]
let r3 = foo<number[]>([]); // number[]
let r4 = foo([1, '']); // {}[]
/*pruned*/;                   // any[]
let r6 = foo<Object[]>([1, '']); // Object[]


function main(): void {}
