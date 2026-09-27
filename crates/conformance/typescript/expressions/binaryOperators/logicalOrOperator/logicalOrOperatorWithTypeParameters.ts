// @target: es2015
function fn1<T, U>(t: T, u: U): void {
    let r1 = t || t;
    let r2: T = t || t;
    let r3 = t || u;
    let r4: {} = t || u;
}

function fn2<T, U/* extends T*/, V/* extends T*/>(t: T, u: U, v: V): void {
    let r1 = t || u;
    //var r2: T = t || u;
    let r3 = u || u;
    let r4: U = u || u;
    let r5 = u || v;
    let r6: {} = u || v;
    //var r7: T = u || v;
}

/*pruned*/;                                                                                             
                    
                        
                            
                                   
 

function main(): void {}
