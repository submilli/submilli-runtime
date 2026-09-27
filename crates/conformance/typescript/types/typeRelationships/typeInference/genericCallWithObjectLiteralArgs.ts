// @target: es2015
function foo<T>(x: { bar: T; baz: T }): { bar: T; baz: T; } {
    return x;
}

let r = foo({ bar: 1, baz: '' }); // error
let r2 = foo({ bar: 1, baz: 1 }); // T = number
/*pruned*/;                           // T = typeof foo
let r4 = foo<Object>({ bar: 1, baz: '' }); // T = Object

function main(): void {}
