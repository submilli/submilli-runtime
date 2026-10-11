// @target: es2015
// @declaration: true

interface Base {
    x: string;
    y: number;
}

interface HelloOrWorld extends Base {
    p1: boolean;
}

interface JustHello extends Base {
    p2: boolean;
}

interface JustWorld extends Base {
    p3: boolean;
}

let hello: "hello" = null as unknown as ("hello");
let world: "world" = null as unknown as ("world");
let helloOrWorld: "hello" | "world" = null as unknown as ("hello" | "world");

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

function main(): void {}
