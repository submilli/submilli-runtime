// @target: es2015
enum E { a, b }

/*pruned*/;                           
/*pruned*/;                             

/*pruned*/;                            
/**/;   
/**/;   
/*pruned*/;
/**/;   
/**/;    
/**/;     
/**/;    
/*pruned*/;
/*pruned*/;

let x2: string = null as unknown as (string);
/**/;   
/**/;   
x2 += true;
x2 += 0;
x2 += '';
x2 += E.a;
x2 += {};
x2 += null;
x2 += null;

let x3: number = null as unknown as (number);
/**/;   
x3 += 0;
x3 += E.a;
x3 += null;
x3 += null;

/*pruned*/;                        
/**/;   
/**/;   
/**/;     
/*pruned*/;
/*pruned*/;

let x5: boolean = null as unknown as (boolean);
/**/;   

let x6: {} = null as unknown as ({});
/**/;   
x6 += '';

/*pruned*/;                              
/**/;   

function main(): void {}
