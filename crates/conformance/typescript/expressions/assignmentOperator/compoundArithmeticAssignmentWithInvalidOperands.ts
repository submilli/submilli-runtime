// @target: es2015
enum E { a, b }

/*pruned*/;                           
/*pruned*/;                             

let x1: boolean = null as unknown as (boolean);
/**/;   
/**/;   
x1 *= true;
x1 *= 0;
x1 *= ''
x1 *= E.a;
x1 *= {};
x1 *= null;
x1 *= null;

let x2: string = null as unknown as (string);
/**/;   
/**/;   
x2 *= true;
x2 *= 0;
x2 *= ''
x2 *= E.a;
x2 *= {};
x2 *= null;
x2 *= null;

let x3: {} = null as unknown as ({});
/**/;   
/**/;   
x3 *= true;
x3 *= 0;
x3 *= ''
x3 *= E.a;
x3 *= {};
x3 *= null;
x3 *= null;

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

let x5: number = null as unknown as (number);
/**/;   
x5 *= true;
x5 *= ''
x5 *= {};

/*pruned*/;                        
/**/;   
/*pruned*/;
/**/;   
/**/;    

function main(): void {}
