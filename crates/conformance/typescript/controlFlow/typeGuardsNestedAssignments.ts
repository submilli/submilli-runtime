// @target: es2015
// @strictNullChecks: true

class Foo {
    x: string = "";
}

/*pruned*/;                                                                    
function getStringOrNumberOrNull(): string | number | null { return null as unknown as (string | number | null); }

function f1(): void {
    /*pruned*/;                                           
    /*pruned*/;                           
                    
     
}

/*pruned*/;          
                                                           
                                                           
                                                        
                            
                     
     
 

function f3(): void {
    /*pruned*/;                                                 
    /*pruned*/;                                 
            
     
}

function f4(): void {
    let x: string | number | null = null as unknown as (string | number | null);
    if (typeof (x = getStringOrNumberOrNull()) === "number") {
        x;
    }
}

// Repro from #8851

const re = /./g
/*pruned*/;                                                                     

/*pruned*/;                               
                                                    
 

function main(): void {}
