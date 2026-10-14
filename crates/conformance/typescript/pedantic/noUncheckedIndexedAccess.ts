// @target: es2015
// @strict: true
// @noUncheckedIndexedAccess: true

/*pruned*/;                                    
// Validate CheckBooleanOnly works - should error
/*pruned*/;                                         

enum NumericEnum1 { A, B, C }
enum NumericEnum2 { A = 0, B = 1 , C = 2 }
enum StringEnum1 { A = "Alpha", B = "Beta" }

const strMap: { [s: string]: boolean } = null as unknown as ({ [s: string]: boolean });

// All of these should be errors
const e1: boolean = strMap["foo"];
const e2: boolean = strMap.bar;
const e3: boolean = strMap[0];
const e4: boolean = strMap[0 as string | number];
const e5: boolean = strMap[0 as string | 0 | 1];
const e6: boolean = strMap[0 as 0 | 1];
const e7: boolean = strMap["foo" as "foo" | "baz"];
const e8: boolean = strMap[NumericEnum1.A];
const e9: boolean = strMap[NumericEnum2.A];
const e10: boolean = strMap[StringEnum1.A];
const e11: boolean = strMap[StringEnum1.A as StringEnum1];
const e12: boolean = strMap[NumericEnum1.A as NumericEnum1];
const e13: boolean = strMap[NumericEnum2.A as NumericEnum2];
/*pruned*/;                              

// Should be OK
const ok1: boolean | undefined = strMap["foo"];
const ok2: boolean | undefined = strMap.bar;

/*pruned*/;                                            
/*pruned*/;                                           
/*pruned*/;                                                   
/*pruned*/;                                            
/*pruned*/;                                         

// Writes don't allow 'undefined'; all should be errors
strMap["baz"] = undefined;
strMap.qua = undefined;
strMap[0] = undefined;
/*pruned*/;                     

// Numeric lookups are unaffected
/*pruned*/;                                                                            
// All of these should be ok
/*pruned*/;                        
/*pruned*/;                                  
/*pruned*/;                                 
/*pruned*/;                                     
/*pruned*/;                                     

// Generics
/*pruned*/;                                                             
                   
                       
 
/*pruned*/;                                                             
                
                        
 
/*pruned*/;                                           
                   
                       
 

// Element access into known properties is ok
const obj1: { x: string, y: number, [key: string]: string | number } = null as unknown as ({ x: string, y: number, [key: string]: string | number });
obj1["x"];
const y = "y";
obj1[y];
let yy = "y";
obj1[yy];
let z = "z";
obj1[z];

// Distributivity cases
const strMapUnion: { [s: string]: boolean } | { [s: string]: number } = null as unknown as ({ [s: string]: boolean } | { [s: string]: number });
// Should error
const f1: boolean | number = strMapUnion["foo"];

// Symbol index signatures
/*pruned*/;                                                 
/*pruned*/;                                                             
/*pruned*/;                       // Should OK
/*pruned*/;               // Should error

// Variadic tuples
/*pruned*/;                                                                                   
/*pruned*/;                                          // Should OK
/*pruned*/;                                            // Should error

// Generic index type
const myRecord1: { a: string; b: string } = null as unknown as ({ a: string; b: string });
const myRecord2: { a: string; b: string, [key: string]: string } = null as unknown as ({ a: string; b: string, [key: string]: string });
/*pruned*/;                                                                           // Should OK
/*pruned*/;                                                                           // Should OK
/*pruned*/;                                                    
                                               
                                                     
  



function main(): void {}
