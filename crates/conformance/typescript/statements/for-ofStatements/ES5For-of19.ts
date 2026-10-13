// @target: es5, es2015
// @ignoreDeprecations: 6.0
// @strict: true
// @alwaysStrict: true, false
for (let v of []) {
    v;
    function foo(): void {
        for (const v of []) {
            v;
        }
    }
}


function main(): void {}
