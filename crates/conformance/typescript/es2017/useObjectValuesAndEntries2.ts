// @target: es5, es2015
// @lib: es5

let o = { a: 1, b: 2 };

for (let x of Object.values(o)) {
    let y = x;
}

let entries = Object.entries(o);

function main(): void {}
