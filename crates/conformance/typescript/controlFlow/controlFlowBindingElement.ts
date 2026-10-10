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

    let foo: string | undefined = "";
    /**/;  
                                                                      
             
    
    foo;  // should be string  
}

{
    const data =  { param: 'value' };

    let foo: string | undefined = "";
    /**/;  
                                         
             
    
    foo;  // should be string | undefined
}

{
    const data =  { param: 'value' };

    let foo: string | undefined = "";
    const {
        param = (() => { return "" + 1 })(),
    } = data;
    
    foo;  // should be string
}

{
    interface Window {
        window: Window;
    }

    let foo: string | undefined = null as unknown as (string | undefined);
    /*pruned*/;               
    window.window = window;

    /*pruned*/;                                                
                                                                     

    foo;  // should be string
}

{
    interface Window {
        window: Window;
    }

    let foo: string | undefined = null as unknown as (string | undefined);
    /*pruned*/;               
    window.window = window;

    /*pruned*/;                                       
                                                                               

    foo;  // should be string
}

{
    interface Window {
        window: Window;
    }

    let foo: string | undefined = null as unknown as (string | undefined);
    /*pruned*/;               
    window.window = window;

    /*pruned*/;                                      
                                                                                                              

    foo;  // should be string | undefined
}


function main(): void {}
