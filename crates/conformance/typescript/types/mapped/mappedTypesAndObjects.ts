// @target: es2015
// @strictNullChecks: true
// @declaration: true

function f1<T>(x: Partial<T>, y: Readonly<T>): void {
    let obj: {} = null as unknown as ({});
    obj = x;
    obj = y;
}

function f2<T>(x: Partial<T>, y: Readonly<T>): void {
    /*pruned*/;                                                               
    /**/;   
    /**/;   
}

function f3<T>(x: Partial<T>): void {
    x = {};
}

// Repro from #12900

/*pruned*/;     
                                
             
             
 

/*pruned*/;                   
           
 

interface Something { name: string, value: string };
/*pruned*/;                
                                                     
 

/*pruned*/;                   
                                            
 

// Repro from #13747

/*pruned*/;    
                                                
 


function main(): void {}
