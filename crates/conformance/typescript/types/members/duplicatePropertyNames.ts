// @target: es2015
// @strict: true
// duplicate property names are an error in all types

/*pruned*/;       
                
                
 

/*pruned*/;       
                  
                  
 

/*pruned*/;         
           
           
 

/**/;    
                
                

                    
                    

                               
                               
 

interface I {
    foo: string;
    foo: string;
}

let a: {
    foo: string;
    foo: string;

    bar: () => {};
    bar: () => {};
} = null as unknown as ({
    foo: string;
    foo: string;

    bar: () => {};
    bar: () => {};
});

let b = {
    foo: '',
    foo: '',
    bar: () => { },
    bar: () => { }
}


function main(): void {}
