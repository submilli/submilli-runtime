// @target: es2015

/*pruned*/;                
                                  
                  
     
 

function f2(x: unknown): void {
    if (typeof x === "function") {
        x;  // Function
    }
}

function f3(x: {}): void {
    if (typeof x === "function") {
        x;  // Function
    }
}

function f4<T>(x: T): void {
    if (typeof x === "function") {
        x;  // T & Function
    }
}

function f5(x: { s: string }): void {
    if (typeof x === "function") {
        x;  // never
    }
}

function f6(x: () => string): void {
    if (typeof x === "function") {
        x;  // () => string
    }
}

function f10(x: string | (() => string)): void {
    if (typeof x === "function") {
        x;  // () => string
    }
    else {
        x;  // string
    }
}

function f11(x: { s: string } | (() => string)): void {
    if (typeof x === "function") {
        x;  // () => string
    }
    else {
        x;  // { s: string }
    }
}

function f12(x: { s: string } | { n: number }): void {
    if (typeof x === "function") {
        x;  // never
    }
    else {
        x;  // { s: string } | { n: number }
    }
}

// Repro from #18238

/*pruned*/;                                                    
                           
                            
                                      
                           
     
 

// Repro from #49316

/*pruned*/;                                                                                           
                                                                  
                                        
                              
     
 

/*pruned*/;                                          
                                               
 


function main(): void {}
