// @target: es2015
// @declaration: true
function takeObject(o: object): void {}
function returnObject(): object {
    return {};
}

let nonPrimitive: object = {};
let primitive: boolean = null as unknown as (boolean);

takeObject(nonPrimitive);
nonPrimitive = returnObject();

takeObject(primitive); // expect error
primitive = returnObject(); // expect error

function returnError(): object {
    let ret = 123;
    return ret; // expect error
}


function main(): void {}
