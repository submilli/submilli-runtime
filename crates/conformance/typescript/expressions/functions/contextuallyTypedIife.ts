// @target: es2015
// @strict: true
// arrow
(jake => { })("build");
// function expression
(function (cats) { })("lol");
// Lots of Irritating Superfluous Parentheses
(function (x) { } ("!"));
((((function (y) { }))))("-");
// multiple arguments
((a, b, c) => { })("foo", 101, false);
// default parameters
((m: number = 10) => m + 1)(12);
((n: number = 10) => n + 1)();
// optional parameters
/*pruned*/;         
/*pruned*/;       
/*pruned*/;             // o should be any
// rest parameters
/*pruned*/;                                        
/*pruned*/;                                             
/*pruned*/;                                      
/*pruned*/;                                                     
// destructuring parameters (with defaults too!)
(({ q }) => q)({ q : 13 });
(({ p = 14 }) => p)({ p : 15 });
(({ r = 17 } = { r: 18 }) => r)({r : 19});
(({ u = 22 } = { u: 23 }) => u)();
// contextually typed parameters.
/*pruned*/;                       
/*pruned*/;                                                   
// missing arguments
/*pruned*/;                                
/*pruned*/;         


function main(): void {}
