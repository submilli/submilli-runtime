// @target: es2015
// @strict: true
// (arg: { x: any, y: any }) => void
/*pruned*/;                    
/*pruned*/;        

// (arg: { x: any, y?: number }) => void
/*pruned*/;                        
/*pruned*/;  
/*pruned*/;        

// (arg: { x?: number, y?: number }) => void
/*pruned*/;                            
/**/;  
/*pruned*/;  
/*pruned*/;  
/*pruned*/;        

// (arg?: { x: number, y: number }) => void
function f4({ x, y } = { x: 0, y: 0 }): void { }
f4();
f4({ x: 1, y: 1 });

// (arg?: { x: number, y?: number }) => void
function f5({ x, y = 0 } = { x: 0 }): void { }
f5();
f5({ x: 1 });
f5({ x: 1, y: 1 });

// (arg?: { x?: number, y?: number }) => void
function f6({ x = 0, y = 0 } = {}): void { }
f6();
f6({});
f6({ x: 1 });
f6({ y: 1 });
f6({ x: 1, y: 1 });

// (arg?: { a: { x?: number, y?: number } }) => void
/*pruned*/;                                               
/**/;
/*pruned*/;   
/*pruned*/;         
/*pruned*/;         
/*pruned*/;               

// (arg: [any, any]) => void
/*pruned*/;                  
/*pruned*/;

// (arg: [number, number]) => void
/*pruned*/;                          
/*pruned*/;

// (arg?: [number, number]) => void
function g3([x, y] = [0, 0]): void { }
g3();
g3([1, 1]);

// (arg?: [number, number]) => void
function g4([x, y = 0] = [0]): void { }
g4();
g4([1, 1]);

// (arg?: [number, number]) => void
function g5([x = 0, y = 0] = []): void { }
g5();
g5([1, 1]);


function main(): void {}
