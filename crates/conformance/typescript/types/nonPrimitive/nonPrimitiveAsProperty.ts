// @target: es2015
// @declaration: true
interface WithNonPrimitive {
    foo: object
}

let a: WithNonPrimitive = { foo: {bar: "bar"} };

let b: WithNonPrimitive = {foo: "bar"}; // expect error


function main(): void {}
