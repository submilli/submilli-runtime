// test262: test/built-ins/RegExp/S15.10.2.10_A4.1_T3.js
// The original's uppercase Cyrillic alphabet loop, kept via parallel arrays.

function main(): void {
  const hex: string[] = [
    "\\u0410", "\\u0411", "\\u0412", "\\u0413", "\\u0414", "\\u0415",
    "\\u0416", "\\u0417", "\\u0418", "\\u0419", "\\u041A", "\\u041B",
    "\\u041C", "\\u041D", "\\u041E", "\\u041F", "\\u0420", "\\u0421",
    "\\u0422", "\\u0423", "\\u0424", "\\u0425", "\\u0426", "\\u0427",
    "\\u0428", "\\u0429", "\\u042A", "\\u042B", "\\u042C", "\\u042D",
    "\\u042E", "\\u042F", "\\u0401",
  ];
  const character: string[] = [
    "А", "Б", "В", "Г", "Д", "Е",
    "Ж", "З", "И", "Й", "К", "Л",
    "М", "Н", "О", "П", "Р", "С",
    "Т", "У", "Ф", "Х", "Ц", "Ч",
    "Ш", "Щ", "Ъ", "Ы", "Ь", "Э",
    "Ю", "Я", "Ё",
  ];
  let result = true;
  for (let i = 0; i < hex.length; i++) {
    const arr = new RegExp(hex[i], "").exec(character[i]);
    if (arr === null) {
      result = false;
    } else {
      const matched = arr.match;
      if (matched !== character[i]) {
        result = false;
      }
    }
  }
  assertSameValue(result, true, "\\uHHHH escapes match the uppercase Cyrillic alphabet");
}
