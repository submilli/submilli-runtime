// @target: es2015
// @declaration: true

/*pruned*/;                  
function f(x: "foo"): number {
    return 0;
}

/*pruned*/;                  
function g(x: "bar"): number {
    return 0;
}

let a = f;
let b = g;

a = b;
b = a;

function main(): void {}
