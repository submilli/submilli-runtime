// @target: es2015
// @strict: true
// @noEmit: true

function f1(a: number, b: number, c: number, d: number, e: number, f: number): void { }
f1(1, 2, 3, 4, ...[5, 6]);
f1(...[1], 2, 3, 4, 5, 6);
f1(1, 2, ...[3, 4], 5, 6);
f1(1, 2, ...[3], 4, ...[5, 6]);
f1(...[1, 2], ...[3, 4], ...[5, 6]);
f1(...(([1, 2])), ...(((([3, 4])))), ...([5, 6]));

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

// dicovered in #52845#issuecomment-1459132562
interface IAction {
    run(event?: unknown): unknown;
}
/*pruned*/;                                          
/*pruned*/;                 // error



function main(): void {}
