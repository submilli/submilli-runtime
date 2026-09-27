// @target: es2015
// @strict: true
// @declaration: true

type ValueOrArray<T> = T | Array<ValueOrArray<T>>;

const a0: ValueOrArray<number> = 1;
const a1: ValueOrArray<number> = [1, [2, 3], [4, [5, [6, 7]]]];

/*pruned*/;                                                                            

/*pruned*/;                         
                             
                                                              
                                                               
      

/*pruned*/;                                                                     

/*pruned*/;       
                    
                               
                                  
  

interface Box<T> { value: T };

type T1 = Box<T1>;
type T2 = Box<Box<T2>>;
type T3 = Box<Box<Box<T3>>>;

function f1(t1: T1, t2: T2, t3: T3): void {
    t1 = t2;
    t1 = t3;
    t2 = t1;
    t2 = t3;
    t3 = t1;
    t3 = t2;
}

type Box1 = Box<Box1> | number;

const b10: Box1 = 42;
const b11: Box1 = { value: 42 };
const b12: Box1 = { value: { value: { value: 42 }}};

type Box2 = Box<Box2 | number>;

const b20: Box2 = 42;  // Error
const b21: Box2 = { value: 42 };
const b22: Box2 = { value: { value: { value: 42 }}};

type RecArray<T> = Array<T | RecArray<T>>;

/*pruned*/;                                                                         
/*pruned*/;                                                                                  
/*pruned*/;                                                                                             

/*pruned*/;           // number[]
/*pruned*/;     // number[]
/*pruned*/;                     // number[]
/*pruned*/;           // (string | number)[]
/*pruned*/;           // (string | number)[]
/*pruned*/;        // Error

/*pruned*/;            // (number | number[])[]
/*pruned*/;      // number[][]
/*pruned*/;            // (string | number)[]
/*pruned*/;            // (string | number)[]
/*pruned*/;         // Error

/*pruned*/;            // number[]
/*pruned*/;      // number[]
/*pruned*/;            // (string | number)[]
/*pruned*/;            // (string | number)[]
/*pruned*/;         // Error

type T10 = T10[];
type T11 = readonly T11[];
type T12 = (T12)[];
type T13 = T13[] | string;
/*pruned*/;                      
/*pruned*/;                                       

type ValueOrArray1<T> = T | ValueOrArray1<T>[];
type ValueOrArray2<T> = T | ValueOrArray2<T>[];

/*pruned*/;                                                                
let ra1: ValueOrArray2<string> = null as unknown as (ValueOrArray2<string>);

/*pruned*/;          // Boom!

type NumberOrArray1<T> = T | ValueOrArray1<T>[];
type NumberOrArray2<T> = T | ValueOrArray2<T>[];

/*pruned*/;                                                                
let ra2: ValueOrArray2<string> = null as unknown as (ValueOrArray2<string>);

/*pruned*/;          // Boom!

// Repro from #33617 (errors are expected)

/*pruned*/;                              

/*pruned*/;                                                         
                                                     
                                  
                       
                                                                                                      
                                                         
       
      
 

/*pruned*/;                                    
           
                                                 
                            
                                                       
                                        
                                 
            
                               
                     
              
                                                               
          
 

/*pruned*/;                                    
                                  
                       
 


function main(): void {}
