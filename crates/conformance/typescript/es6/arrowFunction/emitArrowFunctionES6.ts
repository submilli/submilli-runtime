// @strict: true
// @target:es6
let f1 = () => { }
let f2 = (x: string, y: string) => { }
/*pruned*/;                                    
let f4 = (x: string, y: number, z: number=10) => { }
function foo(func: () => boolean): void { }
foo(() => true);
foo(() => { return false; });

// Binding patterns in arrow functions
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
