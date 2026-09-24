// @target: es2015
let x: string|number = null as unknown as (string|number);

let r1 = typeof x === "string" && typeof x === "string" ? x.substr : x.toFixed;

let r2 = !(typeof x === "string" && typeof x === "string") ? x.toFixed : x.substr;

let r3 = typeof x === "string" || typeof x === "string" ? x.substr : x.toFixed;

let r4 = !(typeof x === "string" || typeof x === "string") ? x.toFixed : x.substr;

function main(): void {}
