// @target: es2015
// @strictNullChecks: true
// @declaration: true

function verifyLibTypes<T, K extends keyof T, U>(): void {
    /*pruned*/;                                          
    /*pruned*/;                                                                          
    /*pruned*/;                                            
    /*pruned*/;                                                                                          
    /*pruned*/;                                          
    /*pruned*/;                                                            
    /*pruned*/;                                              
    /*pruned*/;                                                      
}

type Proxy<T> = {
    get(): T;
    set(value: T): void;
}

/*pruned*/;        
                                
 

/*pruned*/;             
                                                
  

function assign<T>(obj: T, props: Partial<T>): void { }
/*pruned*/;                                                                         
/*pruned*/;                                                                                                      
function mapObject<K extends string, T, U>(obj: Record<K, T>, f: (x: T) => U): Record<K, U> { return null as unknown as (Record<K, U>); }
/*pruned*/;                                                                        

interface Point {
    x: number;
    y: number;
}

interface Shape {
    name: string;
    width: number;
    height: number;
    location: Point;
}

interface PartialShape {
    name?: string;
    width?: number;
    height?: number;
    location?: Point;
}

interface ReadonlyShape {
    readonly name: string;
    readonly width: number;
    readonly height: number;
    readonly location: Point;
}

function f0(s1: Shape, s2: Shape): void {
    assign(s1, { name: "circle" });
    assign(s2, { width: 10, height: 20 });
}

function f1(shape: Shape): void {
    let frozen: ReadonlyShape = null as unknown as (ReadonlyShape);
    let frozen_2: Readonly<Shape> = null as unknown as (Readonly<Shape>);
    /*pruned*/;                  
}

function f2(shape: Shape): void {
    let partial: PartialShape = null as unknown as (PartialShape);
    let partial_2: Partial<Shape> = null as unknown as (Partial<Shape>);
    let partial_3: Partial<Shape> = {};
}

function f3(shape: Shape): void {
    /*pruned*/;                                 // { name: string, location: Point }
}

function f4(): void {
    const rec = { foo: "hello", bar: "world", baz: "bye" };
    const lengths = mapObject(rec, s => s.length);  // { foo: number, bar: number, baz: number }
}

function f5(shape: Shape): void {
    /*pruned*/;              
    /*pruned*/;             
    /*pruned*/;     
}

/*pruned*/;                                    
                                     
                                                          
                                  
 

function main(): void {}
