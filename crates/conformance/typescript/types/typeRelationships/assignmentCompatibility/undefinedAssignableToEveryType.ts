// @target: es2015
class C {
    foo: string;
}
/*pruned*/;                        
interface I {
    foo: string;
}
let ai: I = null as unknown as (I);

enum E { A }
/*pruned*/;                        

let b: number = undefined;
let c: string = undefined;
let d: boolean = undefined;
/*pruned*/;             
/*pruned*/;            
let g: void = undefined;
let h: Object = undefined;
let i: {} = undefined;
let j: () => {} = undefined;
/*pruned*/;                 
let l: (x: number) => string = undefined;
/*pruned*/;    
ai = undefined;
/*pruned*/;    
let m: number[] = undefined;
let n: { foo: string } = undefined;
/*pruned*/;                       
let p: Number = undefined;
let q: String = undefined;

/*pruned*/;                                                 
                  
                  
                  
 
//function foo<T, U extends T, V extends Date>(x: T, y: U, z: V) {
//    x = undefined;
//    y = undefined;
//    z = undefined;
//}

function main(): void {}
