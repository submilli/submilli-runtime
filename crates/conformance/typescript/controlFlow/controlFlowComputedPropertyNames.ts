// @target: es2015
// @strict: true
// @noEmit: true

function f1(obj: Record<string, unknown>, key: string): void {
    if (typeof obj[key] === "string") {
        obj[key].toUpperCase();
    }
}

function f2(obj: Record<string, string | null>, key: string): void {
    if (obj[key] !== null) {
        obj[key].toUpperCase();
    }
    let key2 = key + key;
    if (obj[key2] !== null) {
        obj[key2].toUpperCase();
    }
    const key3 = key + key;
    if (obj[key3] !== null) {
        obj[key3].toUpperCase();
    }
}

type Thing = { a?: string, b?: number, c?: number };

function f3(obj: Thing, key: keyof Thing): void {
    if (obj[key] !== null) {
        if (typeof obj[key] === "string") {
            obj[key].toUpperCase();
        }
        if (typeof obj[key] === "number") {
            obj[key].toFixed();
        }
    }
}

/*pruned*/;                                                                 
                   
                               
     
 


function main(): void {}
