// @target: es2015
// basic uses of optional properties without errors

/*pruned*/;  
                
                 
                    
 

let a: {
    foo: string;
    bar?: number;
    baz? (): string;
} = null as unknown as ({
    foo: string;
    bar?: number;
    baz? (): string;
});

let b = { foo: '' };
let c = { foo: '', bar: 3 };
let d = { foo: '', bar: 3, baz: () => '' };

/*pruned*/;                       

/**/; 
/**/; 
/**/; 

a = b;
a = c;
a = d;

/**/; 
/**/; 

function main(): void {}
