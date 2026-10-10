// @target: es2015
enum E { a, b }

/*pruned*/;                           
let b: void = null as unknown as (void);

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
x2 += undefined;

let x3: number = null as unknown as (number);
/**/;   
x3 += 0;
x3 += E.a;
x3 += null;
x3 += undefined;

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

let x7: void = null as unknown as (void);
/**/;   

function main(): void {}
