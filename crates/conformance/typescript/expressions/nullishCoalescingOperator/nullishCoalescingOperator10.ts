// @target: es2015
// @strict: true

function f(): string | undefined { return null as unknown as (string | undefined); }

let gg = f() ?? 'foo'



function main(): void {}
