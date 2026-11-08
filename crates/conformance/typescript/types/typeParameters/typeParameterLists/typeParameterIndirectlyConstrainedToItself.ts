// @target: es2015
class C<U extends T, T extends U> { }
class C2<T extends U, U extends V, V extends T> { }

interface I<U extends T, T extends U> { }
interface I2<T extends U, U extends V, V extends T> { }

function f<U extends T, T extends U>(): void { }
function f2<T extends U, U extends V, V extends T>(): void { }

/**/;   
                                       
                                                    
                         
                                       
                                                    
   

/*pruned*/;                                
/*pruned*/;                                              

class D<U extends T, T extends V, V extends T> { }

// Repro from #25740

/*pruned*/;                                  
/*pruned*/;                              


function main(): void {}
