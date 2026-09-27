// @strictNullChecks: true
// @target: ES2017

function getStringOrNumber(): string | number { return null as unknown as (string | number); }

function f1(): void {
    let x = getStringOrNumber();
    if (typeof x === "string") {
        let n = function() {
            return x.length;
        }();
    }
}

function f2(): void {
    let x = getStringOrNumber();
    if (typeof x === "string") {
        let n = (function() {
            return x.length;
        })();
    }
}

function f3(): void {
    let x = getStringOrNumber();
    let y: number = null as unknown as (number);
    if (typeof x === "string") {
        /*pruned*/;                            
    }
}

// Repros from #8381

let maybeNumber: number | null = null as unknown as (number | null);
(function () {
    maybeNumber = 1;
})();
maybeNumber++;
if (maybeNumber !== null) {
    maybeNumber++;
}

let test: string | null = null as unknown as (string | null);
if (!test) {
    throw new Error('Test is not defined');
}
(() => {
    test.slice(1); // No error
})();

// Repro from #23565

function f4(): void {
    let v: number = null as unknown as (number);
    (function() {
        v = 1;
    })();
    v;
}

/*pruned*/;          
                                                
                  
                
              
         
                         
 

/*pruned*/;          
                                                
                       
                    
         
                         
 

function main(): void {}
