// @target: es2015
// @strict: true
// @noEmit: true

function f1(obj: Record<string, unknown>, key: string): void {
    if (typeof obj[key] === "string") {
        obj[key].toUpperCase();
    }
}

function f2(obj: Record<string, string | undefined>, key: string): void {
    if (obj[key] !== undefined) {
        obj[key].toUpperCase();
    }
    let key2 = key + key;
    if (obj[key2] !== undefined) {
        obj[key2].toUpperCase();
    }
    const key3 = key + key;
    if (obj[key3] !== undefined) {
        obj[key3].toUpperCase();
    }
}

type Thing = { a?: string, b?: number, c?: number };

function f3(obj: Thing, key: keyof Thing): void {
    if (obj[key] !== undefined) {
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
