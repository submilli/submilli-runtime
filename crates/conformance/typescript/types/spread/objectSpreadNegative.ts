// @target: es5, es2015
// @strictNullChecks: true
let o = { a: 1, b: 'no' }

/// private propagates
class PrivateOptionalX {
    private x?: number;
}
class PublicX {
    public x: number = 42;
}
/*pruned*/;                                         
/*pruned*/;                                                                    
/*pruned*/;                                  
/*pruned*/;            // error, x is private
let optionalString: { sn?: string } = null as unknown as ({ sn?: string });
let optionalNumber: { sn?: number } = null as unknown as ({ sn?: number });
let allOptional: { sn: string | number } = { ...optionalString, ...optionalNumber };
// error, 'sn' is optional in source, required in target

// assignability as target
interface Bool { b: boolean };
interface Str { s: string };
let spread = { ...{ b: true }, ...{s: "foo" } };
spread = { s: "foo" };  // error, missing 'b'
let b = { b: false };
spread = b; // error, missing 's'

// literal repeats are not allowed, but spread repeats are fine
/*pruned*/;                                                   
let duplicatedSpread = { ...o, ...o }
// Note: ignore changes the order that properties are printed
let ignore: { a: number, b: string } =
    { b: 'ignored', ...o }

let o3 = { a: 1, b: 'no' }
let o4 = { b: 'yes', c: true }
let combinedBefore: { a: number, b: string, c: boolean } =
    { b: 'ok', ...o3, ...o4 }
let combinedMid: { a: number, b: string, c: boolean } =
    { ...o3, b: 'ok', ...o4 }
let combinedNested: { a: number, b: boolean, c: string, d: string } =
    { ...{ a: 4, ...{ b: false, c: 'overriden' } }, d: 'actually new', ...{ a: 5, d: 'maybe new' } }
let changeTypeBefore: { a: number, b: string } =
    { a: 'wrong type?', ...o3 };
/*pruned*/;                                                                        
                                                        

// primitives are not allowed, except for falsy ones
let spreadNum = { ...12 };
let spreadSum = { ...1 + 1 };
let spreadZero = { ...0 };
spreadZero.toFixed(); // error, no methods even from a falsy number
let spreadBool = { ...true };
spreadBool.valueOf();
let spreadStr = { ...'foo' };
spreadStr.length; // error, no 'length'
spreadStr.charAt(1); // error, no methods either
// functions are skipped
let spreadFunc = { ...function () { } }
spreadFunc(); // error, no call signature

// write-only properties get skipped
/*pruned*/;                                         
/*pruned*/;        // error, 'b' does not exist

// methods are skipped because they aren't enumerable
/*pruned*/;                     
/*pruned*/;       
/*pruned*/;           
/*pruned*/;  // error 'm' is not in '{ ... c }'

// non primitive
/*pruned*/;                  
/*pruned*/;                
/*pruned*/;  // error 'a' is not in {}


function main(): void {}
