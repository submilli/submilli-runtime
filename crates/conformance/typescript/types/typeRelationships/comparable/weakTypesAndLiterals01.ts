// @target: es2015
// @strict: true
// @declaration: true
// @emitDeclarationOnly: true

type WeakTypes =
    | { optional?: true; }
    | { toLowerCase?(): string }
    | { toUpperCase?(): string, otherOptionalProp?: number };

type LiteralsOrWeakTypes =
    | "A"
    | "B"
    | WeakTypes;

let aOrB: "A" | "B" = null as unknown as ("A" | "B");

/*pruned*/;                              
                      
                   
     
          
                   
     
 

const g = (arg: WeakTypes) => {
    if (arg === "A") {
        return arg;
    }
    else {
        return arg;
    }
}

const h = (arg: LiteralsOrWeakTypes) => {
    if (arg === aOrB) {
        return arg;
    }
    else {
        return arg;
    }
}

const i = (arg: WeakTypes) => {
    if (arg === aOrB) {
        return arg;
    }
    else {
        return arg;
    }
}


function main(): void {}
