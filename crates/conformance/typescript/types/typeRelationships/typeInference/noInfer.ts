// @target: es2015
// @strict: true
// @declaration: true

// NoInfer<T> is erased for primitives

type T00 = NoInfer<string>;
type T01 = NoInfer<string | number | boolean>;
type T02 = NoInfer<undefined>;
type T03 = NoInfer<"foo">;
type T04 = NoInfer<`foo${string}`>;
/*pruned*/;                                         
type T06 = NoInfer<{}>;

// NoInfer<T> is preserved for object types

type T10 = NoInfer<string[]>;
type T11 = NoInfer<{ x: string }>;

// NoInfer<T> is erased if it has no effect

type T20<T> = NoInfer<NoInfer<T>>;
/*pruned*/;                                
/*pruned*/;                                  

// keyof NoInfer<T> is transformed into NoInfer<keyof T>

type T30 = keyof NoInfer<{ a: string, b: string }>;
type T31<T> = keyof NoInfer<T>;
/*pruned*/;                                                      

function foo1<T extends string>(a: T, b: NoInfer<T>): void { }
function foo2<T extends string>(a: T, b: NoInfer<T>[]): void { }
function foo3<T extends string>(a: T, b: NoInfer<T[]>): void { }
function foo4<T extends string>(a: T, b: { x: NoInfer<T> }): void { }
function foo5<T extends string>(a: T, b: NoInfer<{ x: T }>): void { }

foo1('foo', 'foo') // ok
foo1('foo', 'bar') // error
foo2('foo', ['bar']) // error
foo3('foo', ['bar']) // error
foo4('foo', { x: 'bar' }) // error
foo5('foo', { x: 'bar' }) // error

/*pruned*/;                          
/*pruned*/;                                      
function doSomething<T>(value: T, getDefault: () => NoInfer<T>): void { }

/*pruned*/;                                    // ok
/*pruned*/;                                 // ok
/*pruned*/;                                 // error

function assertEqual<T>(actual: T, expected: NoInfer<T>): boolean { return null as unknown as (boolean); }

assertEqual({ x: 1 }, { x: 3 }); // ok
const g = { x: 3, y: 2 };
assertEqual(g, { x: 3 }); // error

/*pruned*/;                                                                                          
function test(value: { x: number; }): number { return null as unknown as (number); }

/*pruned*/;                   // error
test({ x: 1, y: 2 }); // error

type Component<Props> = { props: Props; };
function doWork<Props>(Component: Component<Props>, props: NoInfer<Props>): void { }
const comp: Component<{ foo: number }> = null as unknown as (Component<{ foo: number }>);

doWork(comp, { foo: 42 }); // ok
doWork(comp, {}); // error

/*pruned*/;                                                                                        
/*pruned*/;                         

/*pruned*/;                     
/*pruned*/;       
                                                                              

                    
                                 
     
 
/*pruned*/;        
                                                                             
 


function main(): void {}
