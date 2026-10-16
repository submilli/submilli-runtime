// @target: es2015
/*pruned*/;   
                      
 

let a: object = null as unknown as (object);

/*pruned*/;               
                     
                     
 

if (typeof a === 'number') {
    a.toFixed(); // error, never
}

let b: object | null = null as unknown as (object | null);

if (typeof b === 'object') {
   b.toString(); // ok, object | null
} else {
   b.toString(); // error, never
}


function main(): void {}
