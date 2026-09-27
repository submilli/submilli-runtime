// @target: es2015
// @strict: true
let ab: { a: number, b: number } = null as unknown as ({ a: number, b: number });
let abq: { a: number, b?: number } = null as unknown as ({ a: number, b?: number });
let unused1 = { b: 1, ...ab } // error
let unused2 = { ...ab, ...ab } // ok, overwritten error doesn't apply to spreads
let unused3 = { b: 1, ...abq } // ok, abq might have b: undefined
let unused4 = { ...ab, b: 1 } // ok, we don't care that b in ab is overwritten
let unused5 = { ...abq, b: 1 } // ok
function g(obj: { x: number | null }): { x: number | null; } {
    return { x: 1, ...obj }; // ok, obj might have x: undefined
}
function f(obj: { x: number } | null): { x: number; } {
    return { x: 1, ...obj }; // ok, obj might be undefined
}
function h(obj: { x: number } | { x: string }): { x: number; } | { x: string; } {
    return { x: 1, ...obj } // error
}
function i(b: boolean, t: { command: string, ok: string }): { command: string; ok?: string | null; } {
    return { command: "hi", ...(b ? t : {}) } // ok
}
function j(): { command: string; } {
    return { ...{ command: "hi" } , ...{ command: "bye" } } // ok
}
function k(t: { command: string, ok: string }): { command: string; ok: string; spoiler2: boolean; spoiler: boolean; } {
    return { command: "hi", ...{ spoiler: true }, spoiler2: true, ...t } // error
}

/*pruned*/;                                       
                                                
 
/*pruned*/;                                        
                                             
 



function main(): void {}
