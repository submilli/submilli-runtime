// @target: esnext
// @noEmit: true

// https://github.com/microsoft/TypeScript/pull/41094#issuecomment-716044363
{
    let a: 0 | 1 = 0;
    /*pruned*/;                                     
    /*pruned*/;     
}
{
    let a: 0 | 1 = 1;
    /*pruned*/;                                   
    /*pruned*/;     
}
{
    let a: 0 | 1 | 2 = 1;
    /*pruned*/;                                      
    /*pruned*/;         
}
{
    let a: 0 | 1 = 0;
    /*pruned*/;                                                    
    /*pruned*/;         
}
{
    let a: 0 | 1 = 1;
    /*pruned*/;                                                  
    /*pruned*/;         
}

function main(): void {}
