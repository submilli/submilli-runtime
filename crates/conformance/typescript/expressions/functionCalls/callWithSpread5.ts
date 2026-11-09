// @target: es2015
const x: number = null as unknown as (number);
const nnnu: [number, number, number?] = null as unknown as ([number, number, number?]);
const nntnnnt: [number, number] | [number, number, number] = null as unknown as ([number, number] | [number, number, number]);
function fn(a: number, b: number, bb: number, ...c: number[]): number { return null as unknown as (number); }

fn(...nnnu, x)
fn(...nntnnnt, x)


function main(): void {}
