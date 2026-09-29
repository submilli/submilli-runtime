// @target: es2015
// @noImplicitAny: true
let x = 1
const y = 2
let z = 3
globalThis.x // ok
globalThis.y // should error, no property 'y'
globalThis.z // should error, no property 'z'
globalThis['x'] // ok
globalThis['y'] // should error, no property 'y'
globalThis['z'] // should error, no property 'z'
globalThis.Float64Array // ok
globalThis.Infinity // ok

/*pruned*/;                                                                          // ok
/*pruned*/;                                                                          // error
/*pruned*/;                                                                          // error
/*pruned*/;                                                                         


function main(): void {}
