// @target: es2015
// Type inference infers from indexers in target type, no errors expected

function foo<T>(x: T): T {
    return x;
}

/*pruned*/
/*pruned*/

function other<T>(arg: T): void {
    let b: { [x: string]: T } = {};
    let r2 = foo(b); // T
}

/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/

/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/

/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/


function main(): void {}
