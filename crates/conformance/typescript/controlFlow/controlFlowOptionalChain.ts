// @target: es2015
// @strict: true
// @allowUnreachableCode: false

// assignments in shortcutting chain
/*pruned*/;           
                       
                       
                          
                                     
                       
                       
                          
   

let a: number = null as unknown as (number);
/*pruned*/;
a.toString();

let b: number = null as unknown as (number);
/*pruned*/; 
b.toString();

let c: number = null as unknown as (number);
/**/;     
c.toString();

let d: number = null as unknown as (number);
/*pruned*/; 
d.toString();

// type predicates
/*pruned*/;                                                                                                 
const x: string | number = null as unknown as (string | number);
/*pruned*/;  
                
                                 
         
 
      
      
      
         
 
x;
; 
/**/;

/*pruned*/;                                                                                                      
/*pruned*/;    
                
                                    
          
             
 
      
      
       
          
         
 
x;
;  
/**/; 
/**/;

const o3: { x: 1, y: string } | { x: 2, y: number } | undefined = null as unknown as ({ x: 1, y: string } | { x: 2, y: number } | undefined);
if (o3?.x === 1) {
    o3;
    o3.x;
    o3?.x;
}
else {
    o3;
    o3?.x;
    o3.x;
}
o3;
o3?.x;
o3.x;

const o4: { x?: { y: boolean } } = null as unknown as ({ x?: { y: boolean } });
if (o4.x?.y) {
    o4.x; // { y: boolean }
    o4.x.y; // true
    o4.x?.y; // true
}
else {
    o4.x;
    o4.x?.y;
    o4.x.y;
}
o4.x;
o4.x?.y;
o4.x.y;

const o5: { x?: { y: { z?: { w: boolean } } } } = null as unknown as ({ x?: { y: { z?: { w: boolean } } } });
if (o5.x?.y.z?.w) {
    o5.x;
    o5.x.y;
    o5.x.y.z;
    o5.x.y.z.w; // true
    o5.x.y.z?.w; // true
    o5.x?.y.z.w; // true
    o5.x?.y.z?.w; // true
}
else {
    o5.x;
    o5.x?.y;
    o5.x?.y.z;
    o5.x?.y.z?.w;
    o5.x.y;
    o5.x.y.z.w;
}
o5.x;
o5.x?.y;
o5.x?.y.z;
o5.x?.y.z?.w;
o5.x.y;
o5.x.y.z.w;

/*pruned*/;     
                         
 

/*pruned*/;                     
              
 

/*pruned*/;                                                        
/*pruned*/;   
                  
         
 
      
       
          
         
 
;  
/**/; 
/**/;

// asserts
/*pruned*/;                                                                                                                               
/*pruned*/;                                                                                                                    
/*pruned*/;                                                                                                                                                     
const maybeNever: undefined | (() => never) = null as unknown as (undefined | (() => never));

function f01(x: unknown): void {
    if (!!true) {
        /*pruned*/;   
        x;
    }
    if (!!true) {
        /*pruned*/;        
        x;
    }
    if (!!true) {
        /*pruned*/;              
        /*pruned*/;        
        x;
    }
    if (!!true) {
        maybeNever?.();
        x;
    }
}

type Thing = { foo: string | number, bar(): number, baz: object };

function f10(o: Thing | undefined, value: number): void {
    if (o?.foo === value) {
        o.foo;
    }
    if (o?.["foo"] === value) {
        o["foo"];
    }
    if (o?.bar() === value) {
        o.bar;
    }
    if (o?.foo == value) {
        o.foo;
    }
    if (o?.["foo"] == value) {
        o["foo"];
    }
    if (o?.bar() == value) {
        o.bar;
    }
}

function f11(o: Thing | null, value: number): void {
    if (o?.foo === value) {
        o.foo;
    }
    if (o?.["foo"] === value) {
        o["foo"];
    }
    if (o?.bar() === value) {
        o.bar;
    }
    if (o?.foo == value) {
        o.foo;
    }
    if (o?.["foo"] == value) {
        o["foo"];
    }
    if (o?.bar() == value) {
        o.bar;
    }
}

function f12(o: Thing | undefined, value: number | undefined): void {
    if (o?.foo === value) {
        o.foo;  // Error
    }
    if (o?.["foo"] === value) {
        o["foo"];  // Error
    }
    if (o?.bar() === value) {
        o.bar;  // Error
    }
    if (o?.foo == value) {
        o.foo;  // Error
    }
    if (o?.["foo"] == value) {
        o["foo"];  // Error
    }
    if (o?.bar() == value) {
        o.bar;  // Error
    }
}

function f12a(o: Thing | undefined, value: number | null): void {
    if (o?.foo === value) {
        o.foo;
    }
    if (o?.["foo"] === value) {
        o["foo"];
    }
    if (o?.bar() === value) {
        o.bar;
    }
    if (o?.foo == value) {
        o.foo;  // Error
    }
    if (o?.["foo"] == value) {
        o["foo"];  // Error
    }
    if (o?.bar() == value) {
        o.bar;  // Error
    }
}

function f13(o: Thing | undefined): void {
    if (o?.foo !== undefined) {
        o.foo;
    }
    if (o?.["foo"] !== undefined) {
        o["foo"];
    }
    if (o?.bar() !== undefined) {
        o.bar;
    }
    /*pruned*/;               
              
     
    /*pruned*/;                   
                 
     
    /*pruned*/;                 
              
     
}

function f13a(o: Thing | undefined): void {
    if (o?.foo !== null) {
        o.foo;  // Error
    }
    if (o?.["foo"] !== null) {
        o["foo"];  // Error
    }
    if (o?.bar() !== null) {
        o.bar;  // Error
    }
    /*pruned*/;          
              
     
    /*pruned*/;              
                 
     
    /*pruned*/;            
              
     
}

function f14(o: Thing | null): void {
    if (o?.foo !== undefined) {
        o.foo;
    }
    if (o?.["foo"] !== undefined) {
        o["foo"];
    }
    if (o?.bar() !== undefined) {
        o.bar;
    }
}

function f15(o: Thing | undefined, value: number): void {
    if (o?.foo === value) {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
    if (o?.foo !== value) {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
    if (o?.foo == value) {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
    if (o?.foo != value) {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
}

function f15a(o: Thing | undefined, value: unknown): void {
    if (o?.foo === value) {
        o.foo;  // Error
    }
    else {
        o.foo;  // Error
    }
    if (o?.foo !== value) {
        o.foo;  // Error
    }
    else {
        o.foo;  // Error
    }
    if (o?.foo == value) {
        o.foo;  // Error
    }
    else {
        o.foo;  // Error
    }
    if (o?.foo != value) {
        o.foo;  // Error
    }
    else {
        o.foo;  // Error
    }
}

function f16(o: Thing | undefined): void {
    if (o?.foo === undefined) {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
    if (o?.foo !== undefined) {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
    /*pruned*/;               
                        
     
          
              
     
    /*pruned*/;               
              
     
          
                        
     
}

function f20(o: Thing | undefined): void {
    if (typeof o?.foo === "number") {
        o.foo;
    }
    if (typeof o?.["foo"] === "number") {
        o["foo"];
    }
    if (typeof o?.bar() === "number") {
        o.bar;
    }
    if (o?.baz instanceof Error) {
        o.baz;
    }
}

function f21(o: Thing | null): void {
    if (typeof o?.foo === "number") {
        o.foo;
    }
    if (typeof o?.["foo"] === "number") {
        o["foo"];
    }
    if (typeof o?.bar() === "number") {
        o.bar;
    }
    if (o?.baz instanceof Error) {
        o.baz;
    }
}

function f22(o: Thing | undefined): void {
    if (typeof o?.foo === "number") {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
    if (typeof o?.foo !== "number") {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
    if (typeof o?.foo == "number") {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
    if (typeof o?.foo != "number") {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
}

function f23(o: Thing | undefined): void {
    if (typeof o?.foo === "undefined") {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
    if (typeof o?.foo !== "undefined") {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
    if (typeof o?.foo == "undefined") {
        o.foo;  // Error
    }
    else {
        o.foo;
    }
    if (typeof o?.foo != "undefined") {
        o.foo;
    }
    else {
        o.foo;  // Error
    }
}

/*pruned*/;                                                                    
/*pruned*/;                                                                                          

function f30(o: Thing | undefined): void {
    if (!!true) {
        /*pruned*/;    
        /**/; 
    }
    if (!!true) {
        /*pruned*/;           
        /**/; 
    }
    if (!!true) {
        /*pruned*/;                        
        /**/; 
    }
    if (!!true) {
        /*pruned*/;           
        /**/; 
    }
}

function f40(o: Thing | undefined): void {
    switch (o?.foo) {
        case "abc":
            o.foo;
            break;
        case 42:
            o.foo;
            break;
        case undefined:
            o.foo;  // Error
            break;
        default:
            o.foo;  // Error
            break;
    }
}

function f41(o: Thing | undefined): void {
    switch (typeof o?.foo) {
        case "string":
            o.foo;
            break;
        case "number":
            o.foo;
            break;
        case "undefined":
            o.foo;  // Error
            break;
        default:
            o.foo;  // Error
            break;
    }
}

// Repros from #34570

type Shape =
    | { type: 'rectangle', width: number, height: number }
    | { type: 'circle', radius: number }

function getArea(shape?: Shape): number {
    switch (shape?.type) {
        case 'circle':
            return Math.PI * shape.radius ** 2
        case 'rectangle':
            return shape.width * shape.height
        default:
            return 0
    }
}

type Feature = {
  id: string;
  geometry?: {
    type: string;
    coordinates: number[];
  };
};


function extractCoordinates(f: Feature): number[] {
    if (f.geometry?.type !== 'test') {
        return [];
    }
    return f.geometry.coordinates;
}

// Repro from #35842

interface SomeObject {
    someProperty: unknown;
}

let lastSomeProperty: unknown | undefined = null as unknown as (unknown | undefined);

function someFunction(someOptionalObject: SomeObject | undefined): void {
    if (someOptionalObject?.someProperty !== lastSomeProperty) {
        console.log(someOptionalObject);
        console.log(someOptionalObject.someProperty);  // Error
        lastSomeProperty = someOptionalObject?.someProperty;
    }
}

const someObject: SomeObject = {
    someProperty: 42
};

someFunction(someObject);
someFunction(undefined);

// Repro from #35970

let i = 0;
const arr: { tag: ("left" | "right") }[] = null as unknown as ({ tag: ("left" | "right") }[]);

while (arr[i]?.tag === "left") {
    i += 1;
    if (arr[i]?.tag === "right") {
        console.log("I should ALSO be reachable");
    }
}


// Repro from #51941

type Test5 = {
  main?: {
    childs: Record<string, Test5>;
  };
};

/*pruned*/;                             
                                        
                                         
                   
       
    
               
 

function main(): void {}
