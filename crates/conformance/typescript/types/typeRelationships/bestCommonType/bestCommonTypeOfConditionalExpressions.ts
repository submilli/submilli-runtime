// @target: es2015
// conditional expressions return the best common type of the branches plus contextual type (using the first candidate if multiple BCTs exist)
// no errors expected here

let a: { x: number; y?: number } = null as unknown as ({ x: number; y?: number });
let b: { x: number; z?: number } = null as unknown as ({ x: number; z?: number });

class Base { foo: string; }
class Derived extends Base { bar: string; }
class Derived2 extends Base { baz: string; }
/*pruned*/;                                
/*pruned*/;                                         
/*pruned*/;                                            

let r = true ? 1 : 2;
let r3 = true ? 1 : {};
let r4 = true ? a : b; // typeof a
let r5 = true ? b : a; // typeof b
let r6 = true ? (x: number) => { } : (x: Object) => { }; // returns number => void
let r7: (x: Object) => void = true ? (x: number) => { } : (x: Object) => { }; 
let r8 = true ? (x: Object) => { } : (x: number) => { }; // returns Object => void
/*pruned*/;                                // no error since we use the contextual type in BCT
/*pruned*/;                      

function foo5<T, U>(t: T, u: U): Object {
    return true ? t : u; // BCT is Object
}

function main(): void {}
