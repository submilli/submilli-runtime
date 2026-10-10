// @target: es2015
// @strict: true
// object literal properties are widened to any

let x1 = {
    foo: null,
    bar: undefined
}

let y1 = {
    foo: null,
    bar: {
        baz: null,
        boo: undefined
    }
}

// these are not widened

let u: undefined = undefined;
let n: null = null;

let x2 = {
    foo: n,
    bar: u
}

let y2 = {
    foo: n,
    bar: {
        baz: n,
        boo: u
    }
}

function main(): void {}
