// @target: es2015
// @noImplicitAny: true

// simple case
function simple(f: (a: number, b: number) => void): {} { return null as unknown as ({}); }

simple((a: number, b) => {})
simple((a, b: number) => {})


function main(): void {}
