// @target: es2015
// @allowUnreachableCode: true

/*pruned*/;                                                                                         
let cond: boolean = null as unknown as (boolean);

/**/;   
/*pruned*/;                      
                 
           
 
      
                 
           
 
;  // string | number

function a(): void {
    let x: string | number = null as unknown as (string | number);
    if (cond) {
        x = 42;
    }
    else {
        x = "";
        return;
    }
    x; // number
}
function b(): void {
    let x: string | number = null as unknown as (string | number);
    if (cond) {
        x = 42;
        throw "";
    }
    else {
        x = "";
    }
    x; // string
}
function c<T>(data: string | T): T {
    if (typeof data === 'string') {
        return JSON.parse(data);
    }
    else {
        return data;
    }
}
/*pruned*/;                                            
                                   
                                              
     
          
                    
     
 

interface I<T> {
  p: T;
}
function e(x: I<"A" | "B">): void {
    if (x.p === "A") {
        let a: "A" = (null as unknown as typeof x.p)
    }
}


function main(): void {}
