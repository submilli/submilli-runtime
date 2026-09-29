// @target: es2015
// @strict: true
// @declaration: true

function f0(a: string, b: string): void {
    f0(a, b);
    f1(a, b);
    f2(a, b);
}

function f1(...args: readonly string[]): void {
    /*pruned*/;   // Error
    f1('abc', 'def');
    /*pruned*/;        
    /*pruned*/; 
}

function f2(...args: readonly [string, string]): void {
    /*pruned*/; 
    f1('abc', 'def');
    /*pruned*/;        
    /*pruned*/; 
    f2('abc', 'def');
    /*pruned*/;          // Error
    /*pruned*/; 
}

function f4(...args: readonly string[]): void {
    args[0] = 'abc';  // Error
}


function main(): void {}
