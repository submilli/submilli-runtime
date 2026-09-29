// @target: es2015
// Valid cases

let { foo, foo: bar } = { foo: 1 };
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

// Error cases

let { foo1, foo1 } = { foo1: 10 };
let { foo2, bar2: foo2 } = { foo2: 20, bar2: 220 };
let { bar3: foo3, foo3 } = { foo3: 30, bar3: 330 };
const { foo4, foo4 } = { foo4: 40 };
const { foo5, bar5: foo5 } = { foo5: 50, bar5: 550 };
const { bar6: foo6, foo6 } = { foo6: 60, bar6: 660 };

let [blah1, blah1] = [111, 222];
const [blah2, blah2] = [333, 444];


function main(): void {}
