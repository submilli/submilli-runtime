// @strict: true
// @ignoreDeprecations: 6.0
// @alwaysStrict: true, false
// @target: es6

function f(): (() => number) | undefined {
    if (Math.random()) {
        let arguments = 100;
        return () => arguments;
    }
}

function main(): void {}
