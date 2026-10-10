// @target: es2015
// @strict: true

function f10(x : { kind: false, a: string } | { kind: true, b: string } | { kind: string, c: string }): void {
    if (x.kind === false) {
        x.a;
    }
    else if (x.kind === true) {
        x.b;
    }
    else {
        x.c;
    }
}

function f11(x : { kind: false, a: string } | { kind: true, b: string } | { kind: string, c: string }): void {
    switch (x.kind) {
        case false:
            x.a;
            break;
        case true:
            x.b;
            break;
        default:
            x.c;
    }
}

function f13(x: { a: null; b: string } | { a: string, c: number }): void {
    x = { a: null, b: "foo", c: 4};  // Error
}

function f14<T>(x: { a: 0; b: string } | { a: T, c: number }): void {
    if (x.a === 0) {
        x.b;  // Error
    }
}

type Result<T> = { error?: undefined, value: T } | { error: Error };

function f15(x: Result<number>): void {
    if (!x.error) {
        x.value;
    }
    else {
        x.error.message;
    }
}

f15({ value: 10 });
f15({ error: new Error("boom") });

// Repro from #24193

interface WithError {
    error: Error
    data: null
}

interface WithoutError<Data> {
    error: null
    data: Data
}

type DataCarrier<Data> = WithError | WithoutError<Data>

/*pruned*/;                                           
                                 
                                         
                                       
            
                                          
                                       
     
 

// Repro from #28935

/*pruned*/;                                                                               

/*pruned*/;                   
                  
            
     
          
            
     
 

/*pruned*/;                   
                           
            
     
          
            
     
 

// Repro from #33448

type a = {
    type: 'a',
    data: string
}
type b = {
    type: 'b',
    name: string
}
type c = {
    type: 'c',
    other: string
}

type abc = a | b | c;

/*pruned*/;                               
                               
                     
     
          
                      
     
 

type RuntimeValue =
    | { type: 'number', value: number }
    | { type: 'string', value: string }
    | { type: 'boolean', value: boolean };

/*pruned*/;                                                
                              
                           
     
          
                           
     
 

/*pruned*/;                                                                       
                              
                           
     
          
                           
     
 


function main(): void {}
