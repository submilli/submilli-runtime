// @target: es2015
let x: string | number | boolean = null as unknown as (string | number | boolean);
let cond: boolean = null as unknown as (boolean);

(x = "") || (x = 0);
x; // string | number

x = "";
cond || (x = 0);
x; // string | number

export interface NodeList {
    length: number;
}

export interface HTMLCollection {
    length: number;
}

/*pruned*/;                                                                                        
/*pruned*/;                                                                                                    

type EventTargetLike = {a: string} | HTMLCollection | NodeList;

/*pruned*/;                                
/*pruned*/;                 
                     
 

/*pruned*/;                       
                     
 

/*pruned*/;                                                
                     
 


function main(): void {}
