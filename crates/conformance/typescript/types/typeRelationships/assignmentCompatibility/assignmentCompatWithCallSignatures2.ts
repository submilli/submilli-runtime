// @target: es2015
// void returning call signatures can be assigned a non-void returning call signature that otherwise matches

interface T {
    f(x: number): void;
}
/*pruned*/;                       
let a: { f(x: number): void } = null as unknown as ({ f(x: number): void });

/**/; 
/**/; 

interface S {
    f(x: number): string;
}
/*pruned*/;                       
let a2: { f(x: number): string } = null as unknown as ({ f(x: number): string });
/**/; 
/**/;  
/**/; 
a = a2;

/*pruned*/;        
/*pruned*/;              
/*pruned*/;                          
/*pruned*/;                        
a = { f: () => 1 }
/*pruned*/;               
a = { f: function (x: number) { return ''; } }

// errors
/*pruned*/; 
/*pruned*/;                            
a = () => 1;
a = function (x: number) { return ''; }

interface S2 {
    f(x: string): void;
}
/*pruned*/;                          
let a3: { f(x: string): void } = null as unknown as ({ f(x: string): void });
// these are errors
/**/;  
/**/;  
/*pruned*/;          
/*pruned*/;                            
/**/;  
a = a3;
a = (x: string) => 1;
a = function (x: string) { return ''; }


function main(): void {}
