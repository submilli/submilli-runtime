// @target: ES5, ES2015

// Multiple properties with the same name
let e1 = { a: 0, a: 0 };
let e2 = { a: '', a: '' };
let e3 = { a: 0, a: '' };
let e4 = { a: true, a: false };
let e5 = { a: {}, a: {} };
let e6 = { a: 0, 'a': 0 };
let e7 = { 'a': 0, a: 0 };
let e8 = { 'a': 0, "a": 0 };
let e9 = { 'a': 0, 'a': 0 };
let e10 = { "a": 0, 'a': 0 };
let e11 = { 1.0: 0, '1': 0 };
let e12 = { 0: 0, 0: 0 };
let e13 = { 0: 0, 0: 0 };
let e14 = { 0: 0, 0x0: 0 };
let e14_2 = { 0: 0, 0o0: 0 };
let e15 = { "100": 0, 1e2: 0 };
let e16 = { 0x20: 0, 3.2e1: 0 };
let e17 = { a: 0, b: 1, a: 0 };

// Accessor and property with the same name
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
/*pruned*/;                                   
/*pruned*/;                                     
/*pruned*/;                                      
/*pruned*/;                                                      

// Get and set accessor with mismatched type annotations (only g2 is an error after #43662 implemented)
/*pruned*/;                                                      
/*pruned*/;                                              
/*pruned*/;                                                              

// did you mean colon errors
/**/;     
          
          
         
 


function main(): void {}
