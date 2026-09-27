// @target: es2015
interface X {
    foo(x: number, y: number, ...z: string[]): X;
}

function foo(x: number, y: number, ...z: string[]): void {
}

let a: string[] = null as unknown as (string[]);
let z: number[] = null as unknown as (number[]);
/*pruned*/;                         
/*pruned*/;                            

foo(1, 2, "abc");
/*pruned*/;     
/*pruned*/;            

/*pruned*/;          
/*pruned*/;         
/*pruned*/;                

/*pruned*/;                          
/*pruned*/;                         
/*pruned*/;                                

/*pruned*/;            
/*pruned*/;           
/*pruned*/;                  

/*pruned*/;                              
/*pruned*/;                             
/*pruned*/;                                    

/*pruned*/;            
/*pruned*/;           
/*pruned*/;                  

/*pruned*/;                             

class C {
    constructor(x: number, y: number, ...z: string[]) {
        this.foo(x, y);
        /*pruned*/;          
    }
    foo(x: number, y: number, ...z: string[]): void {
    }
}

class D extends C {
    constructor() {
        super(1, 2);
        /*pruned*/;       
    }
    foo(): void {
        super.foo(1, 2);
        /*pruned*/;           
    }
}


function main(): void {}
