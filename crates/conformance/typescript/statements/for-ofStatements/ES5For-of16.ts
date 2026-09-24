// @target: es5, es2015
// @strict: true
for (let v of []) {
    v;
    for (let v of []) {
        let x = v;
        v++;
    }
}

function main(): void {}
