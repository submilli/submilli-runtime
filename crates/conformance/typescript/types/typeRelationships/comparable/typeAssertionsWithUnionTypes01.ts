// @target: es2015
// @strict: true
interface I1 {
    p1: number
}

/*pruned*/;              
               
 

let x = { p1: 10, p2: 20 };
/*pruned*/;            
let z: I1 = x;

/*pruned*/;            
let b = <number>z;
/*pruned*/;   
/*pruned*/;   


function main(): void {}
