// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the
// other operand.

let a: boolean = null as unknown as (boolean);
let b: string = null as unknown as (string);
/*pruned*/;                                 

// operator *
let r1a1 = null * a;
let r1a2 = null * b;
/*pruned*/;         

let r1b1 = a * null;
let r1b2 = b * null;
/*pruned*/;         

let r1c1 = null * true;
let r1c2 = null * '';
let r1c3 = null * {};

let r1d1 = true * null;
let r1d2 = '' * null;
let r1d3 = {} * null;

// operator /
let r2a1 = null / a;
let r2a2 = null / b;
/*pruned*/;         

let r2b1 = a / null;
let r2b2 = b / null;
/*pruned*/;         

let r2c1 = null / true;
let r2c2 = null / '';
let r2c3 = null / {};

let r2d1 = true / null;
let r2d2 = '' / null;
let r2d3 = {} / null;

// operator %
let r3a1 = null % a;
let r3a2 = null % b;
/*pruned*/;         

let r3b1 = a % null;
let r3b2 = b % null;
/*pruned*/;         

let r3c1 = null % true;
let r3c2 = null % '';
let r3c3 = null % {};

let r3d1 = true % null;
let r3d2 = '' % null;
let r3d3 = {} % null;

// operator -
let r4a1 = null - a;
let r4a2 = null - b;
/*pruned*/;         

let r4b1 = a - null;
let r4b2 = b - null;
/*pruned*/;         

let r4c1 = null - true;
let r4c2 = null - '';
let r4c3 = null - {};

let r4d1 = true - null;
let r4d2 = '' - null;
let r4d3 = {} - null;

// operator <<
/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

/*pruned*/;             
/*pruned*/;           
/*pruned*/;           

/*pruned*/;             
/*pruned*/;           
/*pruned*/;           

// operator >>
/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

/*pruned*/;             
/*pruned*/;           
/*pruned*/;           

/*pruned*/;             
/*pruned*/;           
/*pruned*/;           

// operator >>>
/*pruned*/;           
/*pruned*/;           
/*pruned*/;           

/*pruned*/;           
/*pruned*/;           
/*pruned*/;           

/*pruned*/;              
/*pruned*/;            
/*pruned*/;            

/*pruned*/;              
/*pruned*/;            
/*pruned*/;            

// operator &
/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

/*pruned*/;            
/*pruned*/;          
/*pruned*/;          

/*pruned*/;            
/*pruned*/;          
/*pruned*/;          

// operator ^
/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

/*pruned*/;         
/*pruned*/;         
/*pruned*/;         

/*pruned*/;            
/*pruned*/;          
/*pruned*/;          

/*pruned*/;            
/*pruned*/;          
/*pruned*/;          

// operator |
/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

/*pruned*/;             
/*pruned*/;           
/*pruned*/;           

/*pruned*/;             
/*pruned*/;           
/*pruned*/;           

function main(): void {}
