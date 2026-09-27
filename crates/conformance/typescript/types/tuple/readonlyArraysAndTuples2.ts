// @target: es2015
// @strict: true
// @declaration: true
// @emitDecoratorMetadata: true
// @experimentalDecorators: true

type T10 = string[];
type T11 = Array<string>;
type T12 = readonly string[];
type T13 = ReadonlyArray<string>;

type T20 = [number, number];
type T21 = readonly [number, number];

function f1(ma: string[], ra: readonly string[], mt: [string, string], rt: readonly [string, string]): readonly [string, string] { return null as unknown as (readonly [string, string]); }

/*pruned*/;                                   

/**/;    
          
                            
          
                                             
 


function main(): void {}
