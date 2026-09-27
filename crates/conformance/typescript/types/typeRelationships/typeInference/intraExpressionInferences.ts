// @target: es2015
// @strict: true
// @declaration: true

// Repros from #47599

function callIt<T>(obj: {
    produce: (n: number) => T,
    consume: (x: T) => void
}): void { }

callIt({
    produce: () => 0,
    consume: n => n.toFixed()
});

callIt({
    produce: _a => 0,
    consume: n => n.toFixed(),
});

callIt({
    produce() {
        return 0;
    },
    consume: n => n.toFixed()
});

function callItT<T>(obj: [(n: number) => T, (x: T) => void]): void { }

callItT([() => 0, n => n.toFixed()]);
callItT([_a => 0, n => n.toFixed()]);

// Repro from #25092

interface MyInterface<T> {
    retrieveGeneric: (parameter: string) => T,
    operateWithGeneric: (generic: T) => string
}

/*pruned*/;                                                 

/*pruned*/;                    
                                    
                                                    
   

// Repro #38623

function make<M>(o: { mutations: M,  action: (m: M) => void }): void { }

make({
   mutations: {
       foo() { }
   },
   action: (a) => { a.foo() }
});

// Repro from #38845

function foo<A>(options: { a: A, b: (a: A) => void }): void { }

foo({
    a: () => { return 42 },
    b(a) {},
});

foo({
    a: function () { return 42 },
    b(a) {},
});

foo({
    a() { return 42 },
    b(a) {},
});

// Repro from #38872

type Chain<R1, R2> = {
    a(): R1,
    b(a: R1): R2;
    c(b: R2): void;
};

function test<R1, R2>(foo: Chain<R1, R2>): void {}

test({
    a: () => 0,
    b: (a) => 'a',
    c: (b) => {
        const x: string = b;
    }
});

test({
    a: () => 0,
    b: (a) => a,
    c: (b) => {
        const x: number = b;
    }
});

// Repro from #41712

/*pruned*/;             
                     
 

/*pruned*/;                               
/*pruned*/;                          
                                                              
  

/*pruned*/;                                                          
                                       
                                           
  

/*pruned*/;                                                                                                       

/*pruned*/;             
             
                
                     
                                           
                                          
              
                      
                                             
                                          
             
          
      
                 
                
                                     
                                             
         
     
   

// Repro from #48279

/*pruned*/;                                                                            

/*pruned*/;                                                                                   

/*pruned*/;                                                                        

/*pruned*/;                                                              
/*pruned*/;                                                                        
/*pruned*/;                                               

// Repro from #48466

interface Opts<TParams, TDone, TMapped> {
    fetch: (params: TParams, foo: number) => TDone,
    map: (data: TDone) => TMapped
}

function example<TParams, TDone, TMapped>(options: Opts<TParams, TDone, TMapped>): (params: TParams) => TMapped {
    return (params: TParams) => {
        const data = options.fetch(params, 123)
        return options.map(data)
    }
}

interface Params {
    one: number
    two: string
}

example({
    fetch: (params: Params) => 123,
    map: (number) => String(number)
});

example({
    fetch: (params: Params, foo: number) => 123,
    map: (number) => String(number)
});

example({
    fetch: (params: Params, foo) => 123,
    map: (number) => String(number)
});

// Repro from #45255

/*pruned*/;  
                                                                                                                                                                                                   

const x: "a" | "b" = null as unknown as ("a" | "b");

/**/;   
          
                                 
              
                      
   
  

interface Props<T> {
  a: (x: string) => T;
  b: (arg: T) => void;
}

function Foo<T>(props: Props<T>): null { return null as unknown as (null); }

/**/;
      
                 
                 
                     
      
    
   

/*pruned*/;              
         
                                 
                               
    
                                        

/*pruned*/;               
         
                        
                                    
    
   

/*pruned*/;                    
                        
                              
                              
                                        

/*pruned*/;                           
                    
                         
                         
   

/*pruned*/;                                            
                        
                         
                              
                               
                                                    

/*pruned*/;                                                                   
                    
                          
                         
                         
   

/*pruned*/;                                         
                         
                     
                      
                                                            

/*pruned*/;                                                     
                    
                                                                         
                           
   

/*pruned*/;                     
                         
           
                       
              
                          
      
    
                                                            

/*pruned*/;             
                    
           
                              
              
                               
      
    
   

/*pruned*/;                          
                         
           
                       
                       
              
                          
      
    
                                                                    

/*pruned*/;               
                    
           
                              
                            
              
                               
      
    
   

/*pruned*/;                
        
          
            
                                     
        
      
    
                                
                                        

/*pruned*/;                 
        
          
            
                             
        
      
    
                        
   


function main(): void {}
