// @target: es2015
// @strict: true
// @noUncheckedIndexedAccess: true

const strArray: string[] = null as unknown as (string[]);
const strStrTuple: [string, string] = null as unknown as ([string, string]);

// Declaration forms for array destructuring

// Destructuring from a simple array -> include undefined
const [s1] = strArray;
s1.toString(); // Should error, s1 possibly undefined

// Destructuring a rest element -> do not include undefined
const [...s2] = strArray;
s2.push(undefined); // Should error, 'undefined' not part of s2's element type

// Destructuring a rest element -> do not include undefined
const [, , ...s3] = strArray;
s3.push(undefined); // Should error, 'undefined' not part of s2's element type

// Declaration forms for object destructuring

const strMap: { [s: string]: string } = null as unknown as ({ [s: string]: string });

const { t1 } = strMap;
t1.toString(); // Should error, t1 possibly undefined

const { ...t2 } = strMap;
t2.z.toString(); // Should error

// Test intersections with declared properties
/*pruned*/;                                                                                                                                   
{
    /*pruned*/;                     
    /*pruned*/;  // Should OK
    /*pruned*/;  // Should OK
    /*pruned*/;  // Should error
}

{
    /*pruned*/;                     
    /*pruned*/;  // Should OK
    /*pruned*/;    // Should OK
    /*pruned*/;    // Should error
}

{
    /*pruned*/;                     
    ; 
               // Should OK

    ; 
                 // Should OK

    ; 
                 // Should error
}


let target_string: string = null as unknown as (string);
let target_string_undef: string | undefined = null as unknown as (string | undefined);
let target_string_arr: string[] = null as unknown as (string[]);

// Assignment forms
/*pruned*/;                 // Should error
/*pruned*/;                        // Should OK
/*pruned*/;                            // Should OK

{
    /*pruned*/;                                                                                                                                 
    /*pruned*/;                  // Should OK

    let q: number = null as unknown as (number);
    /*pruned*/;            // Should error
}


function main(): void {}
