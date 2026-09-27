// @target: es2015
// In a typed function call, argument expressions are contextually typed by their corresponding parameter types.
/*pruned*/;                                      
/*pruned*/;                                                     
function baz(x: [string, number, boolean]): void { }

let o = { x: ["string", 1], y: { c: true, d: "world", e: 3 } };
let o1: { x: [string, number], y: { c: boolean, d: string, e: number } } = { x: ["string", 1], y: { c: true, d: "world", e: 3 } };
/**/;    // Not error since x has contextual type of tuple namely [string, number]
/*pruned*/;                                                  // Not error

let array = ["string", 1, true];
let tuple: [string, number, boolean] = ["string", 1, true];
baz(tuple);
baz(["string", 1, true]);

baz(array);                          // Error
baz(["string", 1, true, ...array]);  // Error
/**/;                                // Error because x has an array type namely (string|number)[]

function main(): void {}
