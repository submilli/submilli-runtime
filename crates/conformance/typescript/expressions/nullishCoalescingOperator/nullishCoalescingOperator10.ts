// @target: es2015
// @strict: true

function f(): string | null { return null as unknown as (string | null); }

let gg = f() ?? 'foo'



function main(): void {}
