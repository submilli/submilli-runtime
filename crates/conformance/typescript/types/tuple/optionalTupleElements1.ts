// @target: es2015
// @strict: true
// @declaration: true

type T1 = [number, string, boolean];
type T2 = [number, string, boolean?];
type T3 = [number, string?, boolean?];
type T4 = [number?, string?, boolean?];

/*pruned*/;            
/*pruned*/;            
/*pruned*/;            
/*pruned*/;            

/*pruned*/;                            // Error

function f1(t1: T1, t2: T2, t3: T3, t4: T4): void {
    t1 = t1;
    t1 = t2;  // Error
    t1 = t3;  // Error
    t1 = t4;  // Error
    t2 = t1;
    t2 = t2;
    t2 = t3;  // Error
    t2 = t4;  // Error
    t3 = t1;
    t3 = t2;
    t3 = t3;
    t3 = t4;  // Error
    t4 = t1;
    t4 = t2;
    t4 = t3;
    t4 = t4;
}

let t2: T2 = null as unknown as (T2);
let t3: T3 = null as unknown as (T3);
let t4: T4 = null as unknown as (T4);

t2 = [42, "hello"];
t3 = [42, "hello"];
/*pruned*/;    
t3 = [42];
t4 = [42, "hello"];
/*pruned*/;     
/*pruned*/;           
/*pruned*/;   
t4 = [];


function main(): void {}
