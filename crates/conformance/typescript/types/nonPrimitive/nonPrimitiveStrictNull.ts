// @target: es2015
// @strictNullChecks: true

let a: object = null as unknown as (object);
let b: object | null = null as unknown as (object | null);
let c: object | undefined = null as unknown as (object | undefined);
let d: object | null | undefined = null as unknown as (object | null | undefined);
let e: object | null = null as unknown as (object | null);
a.toString; // error
a = undefined; // error
a = null; // error
a = b; // error
a = c; // error
a = d; // error

e = a; // ok
a = e; // ok

if (typeof b !== 'object') {
    b.toString(); // error, never
}

if (typeof b === 'object') {
    a = b; // error, b is not narrowed
}

if (typeof d === 'object') {
    b = d; // ok
    d.toString(); // error, object | null
} else {
    d.toString(); // error, undefined
}

/*pruned*/;     
                                            
        
                           
 

if (d === null) {
    d.toString(); // error, null
} else {
    d.toString(); // error, object | undefined
}

if (typeof d === 'undefined') {
    d.toString(); // error, undefined
} else {
    d.toString(); // error, object | null
}

/*pruned*/;                         

/*pruned*/;                                                // error
/*pruned*/;                                            // error
/*pruned*/;                                                      // error

interface Blah {
  foo: number;
}

/*pruned*/;                                            // ok


function main(): void {}
