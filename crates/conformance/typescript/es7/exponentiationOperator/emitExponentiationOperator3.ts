// @target:es5, es2015

let temp = 10;

/*pruned*/;    
/*pruned*/;    
(-temp++) ** 3;
(+temp--) ** 3;
/*pruned*/;           
/*pruned*/;           
(-(1 ** temp++)) ** 3;
(-(1 ** temp--)) ** 3;

(-3) ** temp++;
(-3) ** temp--;
/*pruned*/;    
/*pruned*/;    
(+3) ** temp++;
(+3) ** temp--;
/*pruned*/;    
/*pruned*/;    
(-3) ** temp++ ** 2;
(-3) ** temp-- ** 2;
/*pruned*/;         
/*pruned*/;         
(+3) ** temp++ ** 2;
(+3) ** temp-- ** 2;
/*pruned*/;         
/*pruned*/;         

3 ** -temp++;
3 ** -temp--;
/*pruned*/;  
/*pruned*/;  
3 ** (-temp++) ** 2;
3 ** (-temp--) ** 2;
3 ** (+temp++) ** 2;
3 ** (+temp--) ** 2;
/*pruned*/;         
/*pruned*/;         


function main(): void {}
