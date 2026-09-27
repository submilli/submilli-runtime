// @target: es2015
// In the right operand of a || operation, 
// the type of a variable or parameter is narrowed by any type guard in the left operand when false, 
// provided the right operand contains no assignments to the variable or parameter.
function foo(x: number | string): boolean {
    return typeof x !== "string" || x.length === 10; // string
}
function foo2(x: number | string): true | 10 {
    // modify x in right hand operand
    return typeof x !== "string" || ((x = 10) || x); // string | number
}
function foo3(x: number | string): true | "hello" {
    // modify x in right hand operand with string type itself
    return typeof x !== "string" || ((x = "hello") || x); // string | number
}
function foo4(x: number | string | boolean): boolean {
    return typeof x === "string" // string | number | boolean
        || typeof x === "number"  // number | boolean
        || x;   // boolean
}
function foo5(x: number | string | boolean): number | boolean {
    // usage of x or assignment to separate variable shouldn't cause narrowing of type to stop
    let b: number | boolean = null as unknown as (number | boolean);
    return typeof x === "string" // string | number | boolean
        || ((b = x) || (typeof x === "number"  // number | boolean
        || x));   // boolean
}
function foo6(x: number | string | boolean): boolean {
    // Mixing typeguard
    return typeof x === "string" // string | number | boolean
        || (typeof x !== "number" // number | boolean
        ? x // boolean
        : x === 10) // number 
}
function foo7(x: number | string | boolean): string | number | boolean {
    let y: number| boolean | string = null as unknown as (number| boolean | string);
    let z: number| boolean | string = null as unknown as (number| boolean | string);
    // Mixing typeguard narrowing
    return typeof x === "string"
        || ((z = x) // number | boolean
        || (typeof x === "number"
        // change value of x
        ? ((x = 10) && x.toString()) // number | boolean | string
        // do not change value
        : ((y = x) && x.toString()))); // number | boolean | string
}


function main(): void {}
