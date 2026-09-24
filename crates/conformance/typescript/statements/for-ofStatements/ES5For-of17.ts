// @target: es5, es2015
for (let v of []) {
    v;
    for (let v of [v]) {
        let x = v;
        v++;
    }
}

function main(): void {}
