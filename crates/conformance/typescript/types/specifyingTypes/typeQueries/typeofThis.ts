// @target: es2015
// @noImplicitThis: true
// @strict: true

/*pruned*/; 
                  
                   
                                        
     
 

/*pruned*/;  
                                         
                  
                   
                                                 
                                           

                                     
                  

                                       
     
 


function Test2(): void {
    /*pruned*/;               
}

/*pruned*/;                                 
                              
 

/*pruned*/;                                             
                              
 

class Test5 {
    no: number = 1;

    f: () => void = () => {
        // should not capture this.
        /*pruned*/;               
    }
}

/*pruned*/;      
                          
                                  
     
 

/*pruned*/;      
                          
                                  
     
 

const Test8 = () => {
    /*pruned*/;               
}

class Test9 {
    no: number = 0;
    this: number = 0;

    f(): void {
        if (this instanceof Test9D1) {
            /*pruned*/;                  
            /**/;   
        }

        if (this instanceof Test9D2) {
            /*pruned*/;                  
            /**/;   
        }
    }

    g(): void {
        if (this.no === 1) {
            /*pruned*/;                        
        }

        if (this.this === 1) {
            /*pruned*/;                            
        }
    }
}

class Test9D1 {
    f1(): void {}
}

class Test9D2 {
    f2(): void {}
}

class Test10 {
    a?: { b?: string }

    foo(): void {
        /*pruned*/;                             
        if (this.a) {
            /*pruned*/;                                 // should narrow to { b?: string }
            /*pruned*/;                               

            if (this.a.b) {
                /*pruned*/;                                  // should narrow to string
            }
        }
    }
}

/*pruned*/;   
                          
    
                 
                       
                                    

                                 
                                                                  
         
     
 

class Tests12 {
    test1(): void { // OK
        /*pruned*/;             
    }

    test2(): void { // OK
        for (;;) {}
        /*pruned*/;             
    }

    test3(): void { // expected no compile errors
        /*pruned*/;               
        /*pruned*/;             
    }

    test4(): void { // expected no compile errors
        for (const dummy of []) {}
        /*pruned*/;             
    }
}

function main(): void {}
