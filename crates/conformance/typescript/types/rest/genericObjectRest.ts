// @strict: true
// @target: es2015

const a = 'a';

function f1<T extends { a: string, b: number }>(obj: T): void {
    let { ...r0 } = obj;
    let { a: a1, ...r1 } = obj;
    let { a: a2, b: b2, ...r2 } = obj;
    /*pruned*/;                  
    /*pruned*/;                    
    /*pruned*/;                  
}

const sa = Symbol();
const sb = Symbol();

/*pruned*/;                                                          
                                            
 

function f3<T, K1 extends keyof T, K2 extends keyof T>(obj: T, k1: K1, k2: K2): void {
    /*pruned*/;                             
}

type Item = { a: string, b: number, c: boolean };

function f4<K1 extends keyof Item, K2 extends keyof Item>(obj: Item, k1: K1, k2: K2): void {
    /*pruned*/;                             
}


function main(): void {}
