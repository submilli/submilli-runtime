// @target: esnext
// @noEmit: true

// https://github.com/microsoft/TypeScript/pull/41094#issuecomment-716044363
function f(): void { }
{
    let a: 0 | 1 = 0;
    let b: 0 | 1 | 9 = null as unknown as (0 | 1 | 9);
    /*pruned*/;                               
    /*pruned*/;     
}
{
    let a: 0 | 1 = 1;
    let b: 0 | 1 | 9 = null as unknown as (0 | 1 | 9);
    /*pruned*/;                             
    /*pruned*/;     
}
{
    let a: 0 | 1 = 0;
    let b: 0 | 1 | 8 | 9 = null as unknown as (0 | 1 | 8 | 9);
    /*pruned*/;                                              
    /*pruned*/;         
}
{
    let a: 0 | 1 = 1;
    let b: 0 | 1 | 8 | 9 = null as unknown as (0 | 1 | 8 | 9);
    /*pruned*/;                                            
    /*pruned*/;         
}
// same as above but on left of a binary expression
{
    let a: 0 | 1 = 0;
    let b: 0 | 1 | 9 = null as unknown as (0 | 1 | 9);
    /*pruned*/;                                    
    /*pruned*/;     
}
{
    let a: 0 | 1 = 1;
    let b: 0 | 1 | 9 = null as unknown as (0 | 1 | 9);
    /*pruned*/;                                  
    /*pruned*/;     
}
{
    let a: 0 | 1 = 0;
    let b: 0 | 1 | 8 | 9 = null as unknown as (0 | 1 | 8 | 9);
    /*pruned*/;                                                   
    /*pruned*/;         
}
{
    let a: 0 | 1 = 1;
    let b: 0 | 1 | 8 | 9 = null as unknown as (0 | 1 | 8 | 9);
    /*pruned*/;                                                 
    /*pruned*/;         
}
// same as above but on right of a binary expression
{
    let a: 0 | 1 = 0;
    let b: 0 | 1 | 9 = null as unknown as (0 | 1 | 9);
    /*pruned*/;                                    
    /*pruned*/;     
}
{
    let a: 0 | 1 = 1;
    let b: 0 | 1 | 9 = null as unknown as (0 | 1 | 9);
    /*pruned*/;                                  
    /*pruned*/;     
}
{
    let a: 0 | 1 = 0;
    let b: 0 | 1 | 8 | 9 = null as unknown as (0 | 1 | 8 | 9);
    /*pruned*/;                                                   
    /*pruned*/;         
}
{
    let a: 0 | 1 = 1;
    let b: 0 | 1 | 8 | 9 = null as unknown as (0 | 1 | 8 | 9);
    /*pruned*/;                                                 
    /*pruned*/;         
}

function main(): void {}
