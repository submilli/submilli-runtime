// @target: es2015
// @strictNullChecks: true

// Type guards involving type parameters produce intersection types

class C {
    prop: string = "";
}

function f1<T>(x: T): void {
    if (x instanceof C) {
        let v1: T = x;
        let v2: C = x;
        x.prop;
    }
}

function f2<T>(x: T): void {
    if (typeof x === "string") {
        let v1: T = x;
        let v2: string = x;
        x.length;
    }
}

// Repro from #13872

/*pruned*/;                                            
                                 
                             
                                
                                        
                                
         
     
 


function main(): void {}
