// @target: es2015
// @strictNullChecks: true
// @allowUnreachableCode: false
{
    const data =  { param: 'value' };

    /**/;  
                                                                      
             
    
    /*pruned*/;         // should not trigger 'Unreachable code detected.'    
}


{
    const data =  { param: 'value' };

    let foo: string | null = "";
    /**/;  
                                                                      
             
    
    foo;  // should be string  
}

{
    const data =  { param: 'value' };

    let foo: string | null = "";
    /**/;  
                                         
             
    
    foo;  // should be string | undefined
}

{
    const data =  { param: 'value' };

    let foo: string | null = "";
    /**/;  
                                            
             
    
    foo;  // should be string
}

{
    interface Window {
        window: Window;
    }

    let foo: string | null = null as unknown as (string | null);
    /*pruned*/;               
    window.window = window;

    /*pruned*/;                                                
                                                                     

    foo;  // should be string
}

{
    interface Window {
        window: Window;
    }

    let foo: string | null = null as unknown as (string | null);
    /*pruned*/;               
    window.window = window;

    /*pruned*/;                                       
                                                                               

    foo;  // should be string
}

{
    interface Window {
        window: Window;
    }

    let foo: string | null = null as unknown as (string | null);
    /*pruned*/;               
    window.window = window;

    /*pruned*/;                                      
                                                                                                              

    foo;  // should be string | undefined
}


function main(): void {}
