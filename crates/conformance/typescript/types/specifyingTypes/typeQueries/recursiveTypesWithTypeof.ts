// @target: es2015
// The following are errors because of circular references
let c: typeof c = null as unknown as (typeof c);
/*pruned*/;                             
let d: typeof e = null as unknown as (typeof e);
/*pruned*/;                             
let e: typeof d = null as unknown as (typeof d);
/*pruned*/;                             

interface Foo<T> { }
let f: Array<typeof f> = null as unknown as (Array<typeof f>);
/*pruned*/;                             
let f2: Foo<typeof f2> = null as unknown as (Foo<typeof f2>);
/*pruned*/;                              
let f3: Foo<typeof f3>[] = null as unknown as (Foo<typeof f3>[]);
/*pruned*/;                              

// None of these declarations should have any errors!
// Truly recursive types
let g: { x: typeof g; } = null as unknown as ({ x: typeof g; });
let g_2: typeof g.x = null as unknown as (typeof g.x);
let h: () => typeof h = null as unknown as (() => typeof h);
let h_2 = h();
let i: (x: typeof i) => typeof x = null as unknown as ((x: typeof i) => typeof x);
let i_2 = i(i);
/*pruned*/;                                                                                   
/*pruned*/;    

// Same as h, i, j with construct signatures
/*pruned*/;                                                            
/*pruned*/;         
/*pruned*/;                                                                                  
/*pruned*/;           
/*pruned*/;                                                                                              
/*pruned*/;           

// Indexers
/*pruned*/;                                                                                                                   
/*pruned*/;    
/*pruned*/;     

// Hybrid - contains type literals as well as type arguments
// These two are recursive
let hy1: { x: typeof hy1 }[] = null as unknown as ({ x: typeof hy1 }[]);
let hy1_2 = hy1[0].x;
let hy2: { x: Array<typeof hy2> } = null as unknown as ({ x: Array<typeof hy2> });
let hy2_2 = hy2.x[0];

interface Foo2<T, U> { }

// This one should be an error because the first type argument is not contained inside a type literal
let hy3: Foo2<typeof hy3, { x: typeof hy3 }> = null as unknown as (Foo2<typeof hy3, { x: typeof hy3 }>);
/*pruned*/;                               

function main(): void {}
