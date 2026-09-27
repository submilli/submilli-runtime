// @target: es2015
// @strictNullChecks: true
function f(x: { y: string } | null): { y: string } {
    return { y: 123, ...x } // y: string | number
}
f(null)


/*pruned*/;                                 
                     
                                               
 
;  
/**/;  
/**/;  

// spreading nothing but null and undefined is not allowed
const nullAndUndefinedUnion: null | null = null as unknown as (null | null);
let x = { ...nullAndUndefinedUnion, ...nullAndUndefinedUnion };
let y = { ...nullAndUndefinedUnion };


function main(): void {}
