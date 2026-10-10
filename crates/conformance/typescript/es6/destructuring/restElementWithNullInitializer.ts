// @target: es5, es2015
// @strict: true
function foo1([...r] = null): void {
}

function foo2([...r] = undefined): void {
}

function foo3([...r] = {}): void {
}

function foo4([...r] = []): void {
}


function main(): void {}
