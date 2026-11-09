// @target: es2015
// @strict: true
// type of an array is the best common type of its elements (plus its contextual type if it exists)

let a = [1, '']; // {}[]
let b = [1, null]; // number[]
let c = [1, '', null]; // {}[]
let d = [{}, 1]; // {}[]
let e = [{}, Object]; // {}[]

let f = [[], [1]]; // number[][]
let g = [[1], ['']]; // {}[]

let h = [{ foo: 1, bar: '' }, { foo: 2 }]; // {foo: number}[]
let i = [{ foo: 1, bar: '' }, { foo: '' }]; // {}[]

let j = [() => 1, () => '']; // {}[]
let k = [() => 1, () => 1]; // { (): number }[]
let l = [() => 1, () => null]; // { (): any }[]
let m = [() => 1, () => '', () => null]; // { (): any }[]
let n = [[() => 1], [() => '']]; // {}[]

class Base { foo: string; }
class Derived extends Base { bar: string; }
class Derived2 extends Base { baz: string; }
/*pruned*/;                                
/*pruned*/;                                         
/*pruned*/;                                            

/*pruned*/;        
                                                                             
                                                                                   

                                                        
                                           
                                                      
                                                                     
                                                              
                                        
                                                
                                                        
 

/*pruned*/;                   
                
                                        
                              
                          
                                                            
 

function foo<T, U>(t: T, u: U): void {
    let a = [t, t]; // T[]
    let b = [t, null]; // T[]
    let c = [t, u]; // {}[]
    let d = [t, 1]; // {}[]
    let e = [() => t, () => u]; // {}[]
    let f = [() => t, () => u, () => null]; // { (): any }[]
}

function foo2<T extends Base, U extends Derived>(t: T, u: U): void {
    let a = [t, t]; // T[]
    let b = [t, null]; // T[]
    let c = [t, u]; // {}[]
    let d = [t, 1]; // {}[]
    let e = [() => t, () => u]; // {}[]
    let f = [() => t, () => u, () => null]; // { (): any }[]

    /*pruned*/;        // Base[]
    /*pruned*/;           // Derived[]
    /*pruned*/;        // Base[]
    /*pruned*/;           // Derived[]
}

function foo3<T extends Derived, U extends Derived>(t: T, u: U): void {
    let a = [t, t]; // T[]
    let b = [t, null]; // T[]
    let c = [t, u]; // {}[]
    let d = [t, 1]; // {}[]
    let e = [() => t, () => u]; // {}[]
    let f = [() => t, () => u, () => null]; // { (): any }[]

    /*pruned*/;        // Base[]
    /*pruned*/;           // Derived[]
    /*pruned*/;        // Base[]
    /*pruned*/;           // Derived[]
}

function foo4<T extends Base, U extends Base>(t: T, u: U): void {
    let a = [t, t]; // T[]
    let b = [t, null]; // T[]
    let c = [t, u]; // BUG 821629
    let d = [t, 1]; // {}[]
    let e = [() => t, () => u]; // {}[]
    let f = [() => t, () => u, () => null]; // { (): any }[]

    /*pruned*/;        // Base[]
    /*pruned*/;           // Derived[]
    /*pruned*/;        // Base[]
    /*pruned*/;           // Derived[]

    let k: Base[] = [t, u];
}

//function foo3<T extends U, U extends Derived>(t: T, u: U) {
//    var a = [t, t]; // T[]
//    var b = [t, null]; // T[]
//    var c = [t, u]; // {}[]
//    var d = [t, 1]; // {}[]
//    var e = [() => t, () => u]; // {}[]
//    var f = [() => t, () => u, () => null]; // { (): any }[]

//    var g = [t, base]; // Base[]
//    var h = [t, derived]; // Derived[]
//    var i = [u, base]; // Base[]
//    var j = [u, derived]; // Derived[]
//}

//function foo4<T extends U, U extends Base>(t: T, u: U) {
//    var a = [t, t]; // T[]
//    var b = [t, null]; // T[]
//    var c = [t, u]; // BUG 821629
//    var d = [t, 1]; // {}[]
//    var e = [() => t, () => u]; // {}[]
//    var f = [() => t, () => u, () => null]; // { (): any }[]

//    var g = [t, base]; // Base[]
//    var h = [t, derived]; // Derived[]
//    var i = [u, base]; // Base[]
//    var j = [u, derived]; // Derived[]

//    var k: Base[] = [t, u];
//}

function main(): void {}
