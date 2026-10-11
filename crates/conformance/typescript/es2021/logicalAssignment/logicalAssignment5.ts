// @strict: true
// @target: esnext, es2021, es2020, es2015

function foo1 (f?: (a: number) => void): void {
    /*pruned*/;   
    /**/;
}

function foo2 (f?: (a: number) => void): void {
    /*pruned*/;   
    /**/;
}

function foo3 (f?: (a: number) => void): void {
    /*pruned*/;   
    f(42)
}

function bar1 (f?: (a: number) => void): void {
    /*pruned*/;                   
    /**/;
}

function bar2 (f?: (a: number) => void): void {
    /*pruned*/;                   
    /**/;
}

function bar3 (f?: (a: number) => void): void {
    /*pruned*/;                   
    f(42)
}


function main(): void {}
