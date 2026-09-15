// test262: test/built-ins/String/prototype/replaceAll/getSubstitution-0x0024.js

function main(): void {
  const str = "Ninguém é igual a ninguém. Todo o ser humano é um estranho ímpar.";

  assertSameValue(
    str.replaceAll("ninguém", "$"),
    "Ninguém é igual a $. Todo o ser humano é um estranho ímpar.",
    "a lone $ is literal",
  );
  assertSameValue(
    str.replaceAll("é", "$"),
    "Ningu$m $ igual a ningu$m. Todo o ser humano $ um estranho ímpar.",
    "a lone $ is literal at every match",
  );
  assertSameValue(
    str.replaceAll("é", "$ -"),
    "Ningu$ -m $ - igual a ningu$ -m. Todo o ser humano $ - um estranho ímpar.",
    "$ followed by a non-token character is literal",
  );
  assertSameValue(
    str.replaceAll("é", "$$$"),
    "Ningu$$m $$ igual a ningu$$m. Todo o ser humano $$ um estranho ímpar.",
    "$$ collapses to $ and the trailing $ is literal",
  );
}
