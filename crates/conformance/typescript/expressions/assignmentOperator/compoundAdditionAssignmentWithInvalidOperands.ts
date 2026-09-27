// @target: es2015
enum E { a, b }

/*pruned*/;                             

let x1: boolean = null as unknown as (boolean);
/**/;   
x1 += true;
x1 += 0;
x1 += E.a;
x1 += {};
x1 += null;
x1 += null;

let x2: {} = null as unknown as ({});
/**/;   
x2 += true;
x2 += 0;
x2 += E.a;
x2 += {};
x2 += null;
x2 += null;

/*pruned*/;                              
/**/;   
/*pruned*/;
/**/;   
/**/;     
/**/;    
/*pruned*/;
/*pruned*/;

let x4: number = null as unknown as (number);
/**/;   
x4 += true;
x4 += {};

/*pruned*/;                        
/**/;   
/*pruned*/;
/**/;    

function main(): void {}
