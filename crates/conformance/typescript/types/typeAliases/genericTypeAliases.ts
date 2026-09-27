// @target: es2015
type Tree<T> = T | { left: Tree<T>, right: Tree<T> };

let tree: Tree<number> = {
    left: {
        left: 0,
        right: {
            left: 1,
            right: 2
        },
    },
    right: 3
};

type Lazy<T> = T | (() => T);

let ls: Lazy<string> = null as unknown as (Lazy<string>);
ls = "eager";
ls = () => "lazy";

type Foo<T> = T | { x: Foo<T> };
type Bar<U> = U | { x: Bar<U> };

// Deeply instantiated generics
let x: Foo<string> = null as unknown as (Foo<string>);
let y: Bar<string> = null as unknown as (Bar<string>);
x = y;
y = x;

x = "string";
x = { x: "hello" };
x = { x: { x: "world" } };

let z: Foo<number> = null as unknown as (Foo<number>);
z = 42;
z = { x: 42 };
z = { x: { x: 42 } };

type Strange<T> = string;  // Type parameter not used
let s: Strange<number> = null as unknown as (Strange<number>);
s = "hello";

interface AB<A, B> {
    a: A;
    b: B;
}

type Pair<T> = AB<T, T>;

/*pruned*/;                              
                
 

/*pruned*/;                                                         
/**/;   
/**/;   
/*pruned*/;    

/*pruned*/;                               
                                    
                                                    
             
 

/*pruned*/;                               
                                    
                                                    
             
 

// Deeply instantiated generics
/*pruned*/;         
/*pruned*/;         
/**/; 


function main(): void {}
