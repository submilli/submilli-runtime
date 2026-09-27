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

let b: number = null;
let c: string = null;
let d: boolean = null;
/*pruned*/;        
/*pruned*/;       
let g: void = null;
let h: Object = null;
let i: {} = null;
let j: () => {} = null;
/*pruned*/;            
let l: (x: number) => string = null;
/**/;     
ai = null;
/**/;     
let m: number[] = null;
let n: { foo: string } = null;
/*pruned*/;                  
let p: Number = null;
let q: String = null;

/*pruned*/;                                                 
             
             
             
 
//function foo<T, U extends T, V extends Date>(x: T, y: U, z: V) {
//    x = undefined;
//    y = undefined;
//    z = undefined;
//}

function main(): void {}
