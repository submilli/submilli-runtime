// @target: es2015
// @strict: true
// @declaration: true

/*pruned*/;                      // string
/*pruned*/;                      // `${number}
/*pruned*/;                      // `${bigint}
/*pruned*/;                                   // `${T}
/*pruned*/;                                                    // `${Capitalize<T>}`

function f1(a: boolean[], x: `${number}`): void {
    let s = a[x];  // boolean
}

function f2(a: boolean[], x: number | `${number}`): void {
    let s = a[x];  // boolean
}

type T10 = boolean[][`${number}`];  // boolean
type T11 = boolean[][number | `${number}`];  // boolean

type T20<T extends number | `${number}`> = T;
/*pruned*/;                                                

type Container<T> = {
    value: T
}

/*pruned*/;                                                                               

/*pruned*/;                                                                                                      

/*pruned*/;                                                                                                                 

/*pruned*/;                             
/*pruned*/;                          

                                                 
                      
                      
   


function main(): void {}
