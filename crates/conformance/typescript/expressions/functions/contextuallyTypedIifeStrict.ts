// @target: es2015
// @strictNullChecks: true
// arrow
/*pruned*/;            
// function expression
/*pruned*/;                  
// Lots of Irritating Superfluous Parentheses
/*pruned*/;              
/*pruned*/;                   
// multiple arguments
/*pruned*/;                           
// default parameters
((m: number = 10) => m + 1)(12);
((n: number = 10) => n + 1)();
// optional parameters
((j?) => j + 1)(12);
((k?) => k + 1)();
((l, o?) => l + o)(12);
// rest parameters
/*pruned*/;                                        
/*pruned*/;                                             
/*pruned*/;                                      
/*pruned*/;                                                     
// destructuring parameters (with defaults too!)
/*pruned*/;                
/*pruned*/;                     
(({ r = 17 } = { r: 18 }) => r)({r : 19});
(({ u = 22 } = { u: 23 }) => u)();
// contextually typed parameters.
let twelve = (f => f(12))(i => i);
let eleven = (o => o.a(11))({ a: function(n) { return n; } });
// missing arguments
/*pruned*/;                                
/*pruned*/;         


function main(): void {}
