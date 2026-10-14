// @target: es2015
// @strict: true
enum e {
    e1,
    e2
}

// A union type U is a subtype of a type T if each type in U is a subtype of T
/*pruned*/;  
                     
                               
                           
 
interface I2 {
    [x: string]: number;
    // S is union type and each constituent type of S is a subtype of T
    foo: string | number; // error string is not subtype of number
    foo2: e | number; // ok e and number both subtype of number
}

interface I3 {
    [x: string]: string;
    foo: string | number; // error numer is not subtype of string
    foo2: e | number; // error e and number both not subtype of string
}

// error cases
interface I4 {
    [x: string]: boolean;
    foo: string | number;
    foo2: e | number;
}


/*pruned*/;   
                      
                         
                     
 


interface I6 {
    [x: string]: RegExp;
    foo: string | number;
    foo2: e | number;
}


interface I7 {
    [x: string]: { bar: number };
    foo: string | number;
    foo2: e | number;
}


interface I8 {
    [x: string]: number[];
    foo: string | number;
    foo2: e | number;
}


interface I9 {
    [x: string]: I8;
    foo: string | number;
    foo2: e | number;
}

class A { foo: number; }
interface I10 {
    [x: string]: A;
    foo: string | number;
    foo2: e | number;
}

class A2<T> { foo: T; }
interface I11 {
    [x: string]: A2<number>;
    foo: string | number;
    foo2: e | number;
}


/*pruned*/;    
                               
                         
                     
 


/*pruned*/;    
                                
                         
                     
 


enum E2 { A }
interface I14 {
    [x: string]: E2;
    foo: string | number;
    foo2: e | number;
}


function f(): void { }
/*pruned*/;  
                       
 
/*pruned*/;    
                          
                         
                     
 


class c { baz: string }
/*pruned*/;  
                       
 
/*pruned*/;    
                          
                         
                     
 


interface I17<T> {
    [x: string]: T;
    foo: string | number;
    foo2: e | number;
}

interface I19 {
    [x: string]: Object;
    foo: string | number;
    foo2: e | number;
}


interface I20 {
    [x: string]: {};
    foo: string | number;
    foo2: e | number;
}

function main(): void {}
