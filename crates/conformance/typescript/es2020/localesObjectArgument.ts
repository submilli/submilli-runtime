// @target: es2020

const enUS = new Intl.Locale("en-US");
const deDE = new Intl.Locale("de-DE");
const jaJP = new Intl.Locale("ja-JP");

/*pruned*/;            
const num = 1000;
const bigint = 123456789123456789n;
const str = "";

/*pruned*/;                                                    

/*pruned*/;              
/*pruned*/;                  
/*pruned*/;                  
/*pruned*/;                      
/*pruned*/;                          
/*pruned*/;                          

num.toLocaleString(enUS);
num.toLocaleString([deDE, jaJP]);

bigint.toLocaleString(enUS);
bigint.toLocaleString([deDE, jaJP]);

str.toLocaleLowerCase(enUS);
str.toLocaleLowerCase([deDE, jaJP]);
str.toLocaleUpperCase(enUS);
str.toLocaleUpperCase([deDE, jaJP]);
str.localeCompare(str, enUS);
str.localeCompare(str, [deDE, jaJP]);

new Intl.PluralRules(enUS);
new Intl.PluralRules([deDE, jaJP]);
/*pruned*/;                           
Intl.PluralRules.supportedLocalesOf(enUS);
Intl.PluralRules.supportedLocalesOf([deDE, jaJP]);
/*pruned*/;                                          

new Intl.RelativeTimeFormat(enUS);
new Intl.RelativeTimeFormat([deDE, jaJP]);
/*pruned*/;                                  
Intl.RelativeTimeFormat.supportedLocalesOf(enUS);
Intl.RelativeTimeFormat.supportedLocalesOf([deDE, jaJP]);
/*pruned*/;                                                 

new Intl.Collator(enUS);
new Intl.Collator([deDE, jaJP]);
/*pruned*/;                        
Intl.Collator.supportedLocalesOf(enUS);
Intl.Collator.supportedLocalesOf([deDE, jaJP]);

new Intl.DateTimeFormat(enUS);
new Intl.DateTimeFormat([deDE, jaJP]);
/*pruned*/;                              
Intl.DateTimeFormat.supportedLocalesOf(enUS);
Intl.DateTimeFormat.supportedLocalesOf([deDE, jaJP]);
/*pruned*/;                                             

new Intl.NumberFormat(enUS);
new Intl.NumberFormat([deDE, jaJP]);
/*pruned*/;                            
Intl.NumberFormat.supportedLocalesOf(enUS);
/*pruned*/;                                           


function main(): void {}
