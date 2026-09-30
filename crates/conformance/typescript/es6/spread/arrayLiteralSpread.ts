// @target: es2015
// @strict: true
function f0(): void {
    let a = [1, 2, 3];
    let a1 = [...a];
    let a2 = [1, ...a];
    let a3 = [1, 2, ...a];
    let a4 = [...a, 1];
    let a5 = [...a, 1, 2];
    let a6 = [1, 2, ...a, 1, 2];
    let a7 = [1, ...a, 2, ...a];
    let a8 = [...a, ...a, ...a];
}

function f1(): void {
    let a = [1, 2, 3];
    let b = ["hello", ...a, true];
    let b_2: (string | number | boolean)[] = null as unknown as ((string | number | boolean)[]);
}

function f2(): void {
    let a = [...[...[...[...[...[]]]]]];
    let b = [...[...[...[...[...[5]]]]]];
}


function main(): void {}
