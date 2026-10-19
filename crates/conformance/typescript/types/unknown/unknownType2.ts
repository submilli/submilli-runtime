// @target: es2015
// @strict: true

/*pruned*/;                                          
/*pruned*/;                     

type SomeResponse = 'yes' | 'no' | 'idk';
let validate: (x: unknown) => SomeResponse = x => (x === 'yes' || x === 'no') ? x : 'idk'; // No error

const u: unknown = undefined;

/*pruned*/;                                                    
/*pruned*/;                                               

if (u === 5) {
    const y = u.toString(10);
}

if (u === true || u === false) {
    const someBool: boolean = u;
}

if (u === undefined) {
    const undef: undefined = u;
}

if (u === null) {
    const someNull: null = u;
}

/*pruned*/;      
                                       
 

if (!(u === 42)) {
    /*pruned*/;                         
}

if (u !== 42) {
    /*pruned*/;                         
}

if (u == 42) {
    /*pruned*/;                         
}

if (u == true) {
    /*pruned*/;                         
}

if (u == Object) {
    /*pruned*/;                         
}

const aString: string = null as unknown as (string);
const aBoolean: boolean = null as unknown as (boolean);
const aNumber: number = null as unknown as (number);
/*pruned*/;                                          
const anObjectLiteral: { x: number } = null as unknown as ({ x: number });
const aUnion: { x: number } | { y: string } = null as unknown as ({ x: number } | { y: string });
/*pruned*/;                                                                                              
const aFunction: () => number = null as unknown as (() => number);

if (u === aString) {
    let uString: string = u;
}

if (u === aBoolean) {
    let uString: boolean = u;
}

if (u === aNumber) {
    let uNumber: number = u;
}

/*pruned*/;          
                            
 

if (u === anObjectLiteral) {
    /*pruned*/;                    
}

if (u === aUnion) {
    /*pruned*/;                                          
}

/*pruned*/;                
                                                                
 

if (u === aFunction) {
    /*pruned*/;               
}

enum NumberEnum {
    A,
    B,
    C
}

enum StringEnum {
    A = "A",
    B = "B",
    C = "C"
}

/*pruned*/;                                
                            
 

if (u === NumberEnum.A) {
    let a: NumberEnum.A = u
}

if (u === StringEnum.B) {
    let b: StringEnum.B = u
}

function switchTestEnum(x: unknown): void {
    switch (x) {
        case StringEnum.A:
            const a: StringEnum.A = x;
            break;
        case StringEnum.B:
            const b: StringEnum.B = x;
            break;
        case StringEnum.C:
            const c: StringEnum.C = x;
            break;
    }
    /*pruned*/;                           
}

function switchTestCollectEnum(x: unknown): void {
    switch (x) {
        case StringEnum.A:
            const a: StringEnum.A = x;
        case StringEnum.B:
            const b: StringEnum.A | StringEnum.B = x;
        case StringEnum.C:
            const c: StringEnum.A | StringEnum.B | StringEnum.C = x;
            const all: StringEnum = x;
            return;
    }
    /*pruned*/;                           
}

function switchTestLiterals(x: unknown): void {
    /*pruned*/; 
               
                             
                  
               
                             
                  
               
                               
                  
                  
                              
                  
                   
                               
                  
                 
                             
                  
                       
                                       
                  
                  
                                 
                  
                  
                                                 
                  
                           
                                              
                  
     
    /*pruned*/;                           
}

function switchTestObjects(x: unknown, y: () => void, z: { prop: number }): void {
    switch (x) {
        case true:
        case false:
            const bool: boolean = x;
            break;
        case y:
            /*pruned*/;            
            break;
        case z:
            /*pruned*/{}           
            break;
    }
    /*pruned*/;                           
}

function switchResponse(x: unknown): SomeResponse {
    switch (x) {
        case 'yes':
        case 'no':
        case 'idk':
            return x;
        default:
            throw new Error('unknown response');
    }
    // Arguably this should be never.
    /*pruned*/;                           
}

function switchResponseWrong(x: unknown): SomeResponse {
    switch (x) {
        case 'yes':
        case 'no':
        case 'maybe':
            return x; // error
        default:
            throw new Error('Can you repeat the question?');
    }
    // Arguably this should be never.
    /*pruned*/;                           
}

// Repro from #33483

function f2(x: unknown): string | undefined {
  if (x !== undefined && typeof x !== 'string') {
    throw new Error();
  }
  return x;
}

function notNotEquals(u: unknown): void {
    /*pruned*/;              
          
                            
     

    if (u !== NumberEnum.A) { }
    else {
        const a: NumberEnum.A = u;
    }


    if (u !== NumberEnum.A && u !== NumberEnum.B && u !== StringEnum.A) { }
    else {
        const aOrB: NumberEnum.A | NumberEnum.B | StringEnum.A  = u;
    }

    // equivalent to
    if (!(u === NumberEnum.A || u === NumberEnum.B || u === StringEnum.A)) { }
    else {
        const aOrB: NumberEnum.A | NumberEnum.B | StringEnum.A  = u;
    }
}






function main(): void {}
