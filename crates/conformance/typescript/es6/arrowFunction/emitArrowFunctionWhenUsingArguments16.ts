// @strict: true
// @ignoreDeprecations: 6.0
// @alwaysStrict: true, false
// @target: es5, es2015

function f(): (() => string) | undefined {
    let arguments = "hello";
    if (Math.random()) {
        return () => arguments[0];
    }
    let arguments_2 = "world";
}

function main(): void {}
