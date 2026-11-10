// @target: es2015
// checking whether other types are subtypes of type parameters with constraints

function f<T extends U, U, V>(t: T, u: U, v: V): void {
    // ok
    let r = true ? t : u;
    let r_2 = true ? u : t;

    // ok
    let r2 = true ? t : v;
    let r2_2 = true ? v : t;

    // ok
    let r3 = true ? v : u;
    let r3_2 = true ? u : v;
}

function main(): void {}
