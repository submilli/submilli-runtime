// @target: es2020

// https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl#Locale_identification_and_negotiation
const count = 26254.39;
/*pruned*/;                         

function log(locale: string): void {
  /*pruned*/; 
                                                                                                    
    
}

log("en-US");
// expected output: 5/24/2012 26,254.39

log("de-DE");
// expected output: 24.5.2012 26.254,39

// https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/RelativeTimeFormat
const rtf1 = new Intl.RelativeTimeFormat('en', { style: 'narrow' });

console.log(rtf1.format(3, 'quarter'));
//expected output: "in 3 qtrs."

console.log(rtf1.format(-1, 'day'));
//expected output: "1 day ago"

const rtf2 = new Intl.RelativeTimeFormat('es', { numeric: 'auto' });

console.log(rtf2.format(2, 'day'));
//expected output: "pasado mañana"

// https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/DisplayNames
const regionNamesInEnglish = new Intl.DisplayNames(['en'], { type: 'region' });
const regionNamesInTraditionalChinese = new Intl.DisplayNames(['zh-Hant'], { type: 'region' });

console.log(regionNamesInEnglish.of('US'));
// expected output: "United States"

console.log(regionNamesInTraditionalChinese.of('US'));
// expected output: "美國"

const locales1 = ['ban', 'id-u-co-pinyin', 'de-ID'];
/*pruned*/;                                           
/*pruned*/;                                                                      

new Intl.Locale(); // should error
new Intl.Locale(new Intl.Locale('en-US'));

new Intl.DisplayNames(); // TypeError: invalid_argument
new Intl.DisplayNames('en'); // TypeError: invalid_argument
new Intl.DisplayNames('en', {}); // TypeError: invalid_argument
console.log((new Intl.DisplayNames(undefined, {type: 'language'})).of('en-GB')); // "British English"

const localesArg = ["es-ES", new Intl.Locale("en-US")];
console.log((new Intl.DisplayNames(localesArg, {type: 'language'})).resolvedOptions().locale); // "es-ES"
console.log(Intl.DisplayNames.supportedLocalesOf(localesArg)); // ["es-ES", "en-US"]
console.log(Intl.DisplayNames.supportedLocalesOf()); // []
console.log(Intl.DisplayNames.supportedLocalesOf(localesArg, {})); // ["es-ES", "en-US"]


function main(): void {}
