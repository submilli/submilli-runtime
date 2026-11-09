// @target: es2015
// all of the below should be errors

class C<T extends T> { } 
class C2<T, U extends U> { } 

interface I<T extends T> { }
interface I2<T, U extends U> { }

function f<T extends T>(): void { }
function f2<T, U extends U>(): void { }

/**/;   
                          
                             
                         
                          
                             
   

/*pruned*/;                   
/*pruned*/;                       

function main(): void {}
