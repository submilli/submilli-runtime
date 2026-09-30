// @target: es2015

interface C1 {
    (): C1;
    prototype: C1;
    p1: string;
}
interface C2 {
    (): C2;
    prototype: C2;
    p2: number;
}
interface D1 extends C1 {
    prototype: D1;
    p3: number;
}
let str: string = null as unknown as (string);
let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);


/*pruned*/;                     
                
 

/*pruned*/;                     
                
 

/*pruned*/;                     
                
 

/*pruned*/;                          
/*pruned*/;                          
/*pruned*/;                          
/*pruned*/;                                        
/*pruned*/;                      // C1
/*pruned*/;                      // C2
/*pruned*/;                      // D1
/*pruned*/;                      // D1

/*pruned*/;                                        
/*pruned*/;                      // C2
/*pruned*/;                      // D1
/*pruned*/;                      // D1
/*pruned*/;                               // C2 | D1

function main(): void {}
