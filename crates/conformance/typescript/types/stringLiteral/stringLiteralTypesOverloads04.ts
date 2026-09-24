// @strict: true
// @target: es2015
// @declaration: true

function f(x: (p: "foo" | "bar") => "foo"): void { }

f(y => {
    const z = y = "foo";
    return z;
})

function main(): void {}
