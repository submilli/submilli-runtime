// @target: es2015
// @strict: true
// enums are only subtypes of number, any and no other types

enum E { A }
/*pruned*/;  
                     
                 
 


interface I2 {
    [x: string]: number;
    foo: E; // ok
}

// error cases
interface I3 {
    [x: string]: string;
    foo: E;
}


interface I4 {
    [x: string]: boolean;
    foo: E;
}


/*pruned*/;   
                      
           
 


interface I6 {
    [x: string]: RegExp;
    foo: E;
}


interface I7 {
    [x: string]: { bar: number };
    foo: E;
}


interface I8 {
    [x: string]: number[];
    foo: E;
}


interface I9 {
    [x: string]: I8;
    foo: E;
}

class A { foo: number; }
interface I10 {
    [x: string]: A;
    foo: E;
}

class A2<T> { foo: T; }
interface I11 {
    [x: string]: A2<number>;
    foo: E;
}


/*pruned*/;    
                               
           
 


/*pruned*/;    
                                
           
 


enum E2 { A }
interface I14 {
    [x: string]: E2;
    foo: E;
}


function f(): void { }
/*pruned*/;  
                       
 
/*pruned*/;    
                          
           
 


class c { baz: string }
/*pruned*/;  
                       
 
/*pruned*/;    
                          
           
 


interface I17<T> {
    [x: string]: T;
    foo: E;
}


/*pruned*/;                    
                   
           
 


interface I19 {
    [x: string]: Object;
    foo: E; // BUG 831833
}


interface I20 {
    [x: string]: {};
    foo: E; // BUG 831833
}

function main(): void {}
