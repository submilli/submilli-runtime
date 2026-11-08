// @target: es2015
// @strict: true

function f0<T extends { a: string, b: string }>(obj: Pick<T, Extract<keyof T, 'b'>>): void {
    obj.b;
}

function f1<T extends { a: string, b: string }>(obj: Pick<T, Exclude<keyof T, 'a'>>): void {
    obj.b;
}

function f2<T extends { a: string, b: string }, U extends { b: string, c: string }>(obj: Pick<T | U, keyof (T | U)>): void {
    obj.b;
}

/*pruned*/;                                                                                                                 
          
          
          
 

function f4<T extends { a: string, b: string }>(obj: Record<Exclude<keyof T, 'b'> | 'c', string>): void {
    obj.a;
    obj.c;
}

// Repro from #28821

type TargetProps = {
    foo: string,
    bar: string
};

/*pruned*/;                                                  
                                     
             
  


function main(): void {}
