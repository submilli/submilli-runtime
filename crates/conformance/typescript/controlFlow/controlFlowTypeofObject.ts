// @target: es2015
// @strict: true
// @declaration: true

/*pruned*/;                      

function f1(x: unknown): void {
    if (!x) {
        return;
    }
    if (typeof x === 'object') {
        /**/;  
    }
}

function f2(x: unknown): void {
    if (x === null) {
        return;
    }
    if (typeof x === 'object') {
        /**/;  
    }
}

function f3(x: unknown): void {
    if (x == null) {
        return;
    }
    if (typeof x === 'object') {
        /**/;  
    }
}

function f4(x: unknown): void {
    if (x == null) {
        return;
    }
    if (typeof x === 'object') {
        /**/;  
    }
}

function f5(x: unknown): void {
    if (!!true) {
        if (!x) {
            return;
        }
    }
    else {
        if (x === null) {
            return;
        }
    }
    if (typeof x === 'object') {
        /**/;  
    }
}

function f6(x: unknown): void {
    if (x === null) {
        x;
    }
    else {
        x;
        if (typeof x === 'object') {
            /**/;  
        }
    }
    if (typeof x === 'object') {
        /**/;    // Error
    }
}


function main(): void {}
