// @target: es2015
// @strict: true

// Test that callback parameters are related covariantly

interface P<T> {
    then(cb: (value: T) => void): void;
};

interface A { a: string }
/*pruned*/;                        

/*pruned*/;                          
          
                    
 

/*pruned*/;                                      
          
                    
 

interface AList1 {
    forEach(cb: (item: A) => void): void;
}

/*pruned*/;       
                                         
 

/*pruned*/;                               
          
                    
 

interface AList2 {
    forEach(cb: (item: A) => boolean): void;
}

interface BList2 {
    forEach(cb: (item: A) => void): void;
}

function f12(a: AList2, b: BList2): void {
    a = b;
    b = a;  // Error
}

interface AList3 {
    forEach(cb: (item: A) => void): void;
}

/*pruned*/;       
                                                       
 

/*pruned*/;                               
          
                    
 

interface AList4 {
    forEach(cb: (item: A) => A): void;
}

/*pruned*/;       
                                      
 

/*pruned*/;                               
          
                    
 

// Repro from #51620

type Bivar<T> = { set(value: T): void }

let bu: Bivar<unknown> = null as unknown as (Bivar<unknown>);
let bs: Bivar<string> = null as unknown as (Bivar<string>);
bu = bs;
bs = bu;

let bfu: Bivar<(x: unknown) => void> = null as unknown as (Bivar<(x: unknown) => void>);
let bfs: Bivar<(x: string) => void> = null as unknown as (Bivar<(x: string) => void>);
bfu = bfs;
bfs = bfu;

type Bivar1<T> = { set(value: T): void }
type Bivar2<T> = { set(value: T): void }

let b1fu: Bivar1<(x: unknown) => void> = null as unknown as (Bivar1<(x: unknown) => void>);
let b2fs: Bivar2<(x: string) => void> = null as unknown as (Bivar2<(x: string) => void>);
b1fu = b2fs;
b2fs = b1fu;

type SetLike<T> = { set(value: T): void, get(): T }

let sx: SetLike1<(x: unknown) => void> = null as unknown as (SetLike1<(x: unknown) => void>);
let sy: SetLike1<(x: string) => void> = null as unknown as (SetLike1<(x: string) => void>);
sx = sy;  // Error
sy = sx;

type SetLike1<T> = { set(value: T): void, get(): T }
type SetLike2<T> = { set(value: T): void, get(): T }

let s1: SetLike1<(x: unknown) => void> = null as unknown as (SetLike1<(x: unknown) => void>);
let s2: SetLike2<(x: string) => void> = null as unknown as (SetLike2<(x: string) => void>);
s1 = s2;  // Error
s2 = s1;


function main(): void {}
