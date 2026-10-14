// @strict: true
// @target:es5, es2015
let f1 = () => {
    /*pruned*/;  
};

let f2 = (x: string) => {
    /*pruned*/;  
}

function foo(func: () => boolean): void { }
foo(() => {
    /*pruned*/;    
    return true;
});


function main(): void {}
