// @target: esnext
// @noEmit: true
// @strictBuiltinIteratorReturn: *

const array: number[] = null as unknown as (number[]);
/*pruned*/;                                                               
/*pruned*/;                                               

const i0 = array[Symbol.iterator]();
const i1 = array.values();
const i2 = array.keys();
const i3 = array.entries();
for (const x of array);

/*pruned*/;                       
/*pruned*/;             
/*pruned*/;           
/*pruned*/;              
/*pruned*/;          

/*pruned*/;                       
/*pruned*/;             
/*pruned*/;            
/*pruned*/;               
/*pruned*/;          

/*pruned*/;                                                                                     
/*pruned*/;                                                                                   
/*pruned*/;                                                                                           
const i15: Iterable<number, null> = null as unknown as (Iterable<number, null>);
/*pruned*/;                                                                   
const i17: Iterable<number, boolean> = null as unknown as (Iterable<number, boolean>);


function main(): void {}
