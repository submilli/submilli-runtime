// @target: es2015
// @strict: true, false

function f1(a?: boolean): void {
    /*pruned*/;

    if (a === false) {
        console.log(a);
    }
}
f1(false);

function f2(): void {
    /*pruned*/;                     
    /**/;   
    /*pruned*/;   
                       
     
}


function main(): void {}
