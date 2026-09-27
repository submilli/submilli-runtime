// @target: es2015
interface Runnable {
    isRunning: boolean;
}

/*pruned*/;                              
                             
 

let x: string = null as unknown as (string);
/*pruned*/;                                                             

let b: boolean = null as unknown as (boolean);
/*pruned*/; 
/*pruned*/;    
/*pruned*/;     
b = "foo" === "bar";
b = "bar" === x;
b = x === "bar";
/*pruned*/;     
/*pruned*/;     

/*pruned*/; 
/*pruned*/;    
/*pruned*/;     
b = "foo" !== "bar";
b = "bar" !== x;
b = x !== "bar";
/*pruned*/;     
/*pruned*/;     


function main(): void {}
