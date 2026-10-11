// @target: es2015
// without strict null checks, none of these should be an error
let ab: { a: number, b: number } = null as unknown as ({ a: number, b: number });
let abq: { a: number, b?: number } = null as unknown as ({ a: number, b?: number });
let unused1 = { b: 1, ...ab }
let unused2 = { ...ab, ...ab }
let unused3 = { b: 1, ...abq }

function g(obj: { x: number | undefined }): { x: number | undefined; } {
    return { x: 1, ...obj };
}
function h(obj: { x: number }): { x: number; } {
    return { x: 1, ...obj };
}


function main(): void {}
