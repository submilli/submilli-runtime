// @strictNullChecks: true
// @target: es5, es2015
let o = { a: 1, b: 'no' }
let o2 = { b: 'yes', c: true }
let swap = { a: 'yes', b: -1 };

let addAfter: { a: number, b: string, c: boolean } =
    { ...o, c: false }
let addBefore: { a: number, b: string, c: boolean } =
    { c: false, ...o }
let override: { a: number, b: string } =
    { ...o, b: 'override' }
let nested: { a: number, b: boolean, c: string } =
    { ...{ a: 3, ...{ b: false, c: 'overriden' } }, c: 'whatever' }
let combined: { a: number, b: string, c: boolean } =
    { ...o, ...o2 }
let combinedAfter: { a: number, b: string, c: boolean } =
    { ...o, ...o2, b: 'ok' }
let combinedNestedChangeType: { a: number, b: boolean, c: number } =
    { ...{ a: 1, ...{ b: false, c: 'overriden' } }, c: -1 }
let propertyNested: { a: { a: number, b: string } } =
    { a: { ... o } }
// accessors don't copy the descriptor
// (which means that readonly getters become read/write properties)
/*pruned*/;                        
/*pruned*/;                           
                   
/*pruned*/;   

// functions result in { }
let spreadFunc = { ...(function () { }) };

type Header = { head: string, body: string, authToken: string }
/*pruned*/;                                                                              
            
                       
                  
                                     
     
 
// boolean && T results in Partial<T>
function conditionalSpreadBoolean(b: boolean) : { x: number, y: number } {
    let o = { x: 12, y: 13 }
    o = {
        ...o,
        ...b && { x: 14 }
    }
    let o2 = { ...b && { x: 21 }}
    return o;
}
function conditionalSpreadNumber(nt: number): { x: number, y: number } {
    let o = { x: 15, y: 16 }
    o = {
        ...o,
        ...nt && { x: nt }
    }
    let o2 = { ...nt && { x: nt }}
    return o;
}
function conditionalSpreadString(st: string): { x: string, y: number } {
    let o = { x: 'hi', y: 17 }
    o = {
        ...o,
        ...st && { x: st }
    }
    let o2 = { ...st && { x: st }}
    return o;
}

// any results in any
/*pruned*/;                                  
/*pruned*/;                     

// methods are not enumerable
/*pruned*/;                     
/*pruned*/;       
/*pruned*/;                          

// own methods are enumerable
/*pruned*/;                                                                      
/*pruned*/;  

// new field's type conflicting with existing field is OK
let changeTypeAfter: { a: string, b: string } =
    { ...o, a: 'wrong type?' }
let changeTypeBoth: { a: string, b: number } =
    { ...o, ...swap };

// optional
/*pruned*/;        
                                     
                                   
                                    
                                            
                                                                                                                             
                                                                                                                                           
                                                                                         

                        
                                                                              
                                                     
                                                                       
                                               
 
// shortcut syntax
let a = 12;
let shortCutted: { a: number, b: string } = { ...o, a }
// non primitive
/*pruned*/;                               

// generic spreads

/*pruned*/;                                            
                                    
 

/*pruned*/;                                                                 
                                                
/*pruned*/;                                        
                                     
/*pruned*/;                                    
                                  
/*pruned*/;                                                       
                                            

function genericSpread<T, U>(t: T, u: U, v: T | U, w: T | { s: string }, obj: { x: number }): void {
    let x01 = { ...t };
    let x02 = { ...t, ...t };
    let x03 = { ...t, ...u };
    let x04 = { ...u, ...t };
    let x05 = { a: 5, b: 'hi', ...t };
    let x06 = { ...t, a: 5, b: 'hi' };
    let x07 = { a: 5, b: 'hi', ...t, c: true, ...obj };
    let x09 = { a: 5, ...t, b: 'hi', c: true, ...obj };
    let x10 = { a: 5, ...t, b: 'hi', ...u, ...obj };
    let x11 = { ...v };
    let x12 = { ...v, ...obj };
    let x13 = { ...w };
    let x14 = { ...w, ...obj };
    let x15 = { ...t, ...v };
    let x16 = { ...t, ...w };
    let x17 = { ...t, ...w, ...obj };
    let x18 = { ...t, ...v, ...w };
}


function main(): void {}
