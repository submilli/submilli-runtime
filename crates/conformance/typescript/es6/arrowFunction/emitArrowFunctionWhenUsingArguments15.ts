// @strict: true
// @ignoreDeprecations: 6.0
// @alwaysStrict: true, false
// @target: es5, es2015

function f(): (() => number) | undefined {
    let arguments = "hello";
    if (Math.random()) {
        const arguments = 100;
        return () => arguments;
    }
}

function main(): void {}
