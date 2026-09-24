// @target: es5, es2015
// @strict: true
for (let v of []) {
    v;
    for (const v of []) {
        let x = v;
    }
}

function main(): void {}
