// @target: es2015
// @strict: true
// @declaration: true

export type Segment = [length: number, count: number];

export type SegmentAnnotated = [
    /** 
     * Size of message buffer segment handles
     */
    length: number,
    /**
     * Number of segments handled at once
     */
    count: number
];

let a: Segment = null as unknown as (Segment);
let b: SegmentAnnotated = null as unknown as (SegmentAnnotated);
let c: [number, number] = null as unknown as ([number, number]);
let d: [a: number, b: number] = null as unknown as ([a: number, b: number]);

a = b;
a = c;
a = d;

b = a;
b = c;
b = d;

c = a;
c = b;
c = d;

d = a;
d = b;
d = c;

/*pruned*/;                                                                      

/*pruned*/;                                           

/*pruned*/;                                               

/*pruned*/;                                                               
                       
 


/*pruned*/;                                                 

export function readSegment([length, count]: [number, number]): void {}

// documenting binding pattern behavior (currently does _not_ generate tuple names)
/*pruned*/;                                                         

export type RecursiveTupleA = [initial: string, next: RecursiveTupleA];

export type RecursiveTupleB = [first: string, ptr: RecursiveTupleB];

let q: RecursiveTupleA = null as unknown as (RecursiveTupleA);
let r: RecursiveTupleB = null as unknown as (RecursiveTupleB);

q = r;
r = q;

/*pruned*/;                                                         
/*pruned*/;                                              

/*pruned*/;                                             
/*pruned*/;                                               

/**/; 
/**/; 

/*pruned*/;                                                               
/*pruned*/;                                                                            
/*pruned*/;                                                                                                                        

/*pruned*/;                                                           // one tuple with captures arguments as first member
/*pruned*/;                                               // captured arguments list re-spread


function main(): void {}
