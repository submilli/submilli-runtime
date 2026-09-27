// @target: es2015
// @allowUnreachableCode: true

interface I {
    id: number;
}

class C implements I {
    id: number;
    name: string;
}

class C2 extends C {
    valid: boolean;
}

/*pruned*/;
              
                  
                    
 

function F(x: string): number { return 42; }
function F2(x: number): boolean { return x < 42; }

/*pruned*/;  
                    
                     
     

                                                                  
 

/*pruned*/;  
                    
                   
     

                                                                  
 

// literals
if (true) { }
while (true) { }
do { }while(true)

if (null) { }
while (null) { }
do { }while(null)

if (null) { }
while (null) { }
do { }while(null)

if (0.0) { }
while (0.0) { }
do { }while(0.0)

if ('a string') { }
while ('a string') { }
do { }while('a string')

if ('') { }
while ('') { }
do { }while('')

if (/[a-z]/) { }
while (/[a-z]/) { }
do { }while(/[a-z]/)

if ([]) { }
while ([]) { }
do { }while([])

if ([1, 2]) { }
while ([1, 2]) { }
do { }while([1, 2])

if ({}) { }
while ({}) { }
do { }while({})

if ({ x: 1, y: 'a' }) { }
while ({ x: 1, y: 'a' }) { }
do { }while({ x: 1, y: 'a' })

if (() => 43) { }
while (() => 43) { }
do { }while(() => 43)

if (new C()) { }
while (new C()) { }
do { }while(new C())

/*pruned*/;        
/*pruned*/;           
/*pruned*/;            

// references
let a = true;
if (a) { }
while (a) { }
do { }while(a)

let b = null;
if (b) { }
while (b) { }
do { }while(b)

let c = null;
if (c) { }
while (c) { }
do { }while(c)

let d = 0.0;
if (d) { }
while (d) { }
do { }while(d)

let e = 'a string';
if (e) { }
while (e) { }
do { }while(e)

let f = '';
if (f) { }
while (f) { }
do { }while(f)

let g = /[a-z]/
if (g) { }
while (g) { }
do { }while(g)

let h = [];
if (h) { }
while (h) { }
do { }while(h)

let i = [1, 2];
if (i) { }
while (i) { }
do { }while(i)

let j = {};
if (j) { }
while (j) { }
do { }while(j)

let k = { x: 1, y: 'a' };
if (k) { }
while (k) { }
do { }while(k)

/*pruned*/;                                
/*pruned*/;  
/*pruned*/;     
/*pruned*/;      

/*pruned*/;
/*pruned*/;   
/*pruned*/;    




function main(): void {}
