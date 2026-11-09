// @target: es2015
// @strict: true
// members N and M of types S and T have the same name, same accessibility, same optionality, and N is assignable M
// numeric named properties work correctly, no errors expected

class S { 1: string; }
class T { 1.: string; }
/*pruned*/;                       
/*pruned*/;                       

interface S2 { 1: string; bar?: string }
interface T2 { 1.0: string; baz?: string }
let s2: S2 = null as unknown as (S2);
let t2: T2 = null as unknown as (T2);

let a: { 1.: string; bar?: string } = null as unknown as ({ 1.: string; bar?: string });
let b: { 1.0: string; baz?: string } = null as unknown as ({ 1.0: string; baz?: string });

let a2 = { 1.0: '' };
let b2 = { 1: '' };

/**/; 
/**/; 
/**/;  
/**/;  

s2 = t2;
t2 = s2;
/**/;  
s2 = b;
s2 = a2;

a = b;
b = a;
/**/; 
a = s2;
a = a2;

a2 = b2;
b2 = a2;
a2 = b;
a2 = t2;
/**/;  


function main(): void {}
