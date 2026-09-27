// @target: es2015
// @strictNullChecks: true

function f1(): void {
    let x: string | number = 1;
    x;
    let y: string | null = "";
    y;
}

function f2(): void {
    let [x]: [string | number] = [1];
    x;
    let [y]: [string | null] = [""];
    y;
    /*pruned*/;                            
    ; 
}

function f3(): void {
    let [x]: (string | number)[] = [1];
    x;
    let [y]: (string | null)[] = [""];
    y;
    /*pruned*/;                              
    ; 
}

/*pruned*/;          
                                                 
      
                                                
      
                                                       
      
 

/*pruned*/;          
                                                  
      
                                                 
      
                                                        
      
 

/*pruned*/;          
                                            
      
                                          
      
                                               
      
 

/*pruned*/;          
                                              
                                                    
      
 


function main(): void {}
