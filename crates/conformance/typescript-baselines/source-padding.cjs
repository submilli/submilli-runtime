const ts = require("typescript");

// Pruning preserves offsets with spaces until all compiler passes have finished.
// Remove that padding for the staged source without changing multiline literals.
function trimSourcePadding(source) {
  const tree = ts.createSourceFile("case.ts", source, ts.ScriptTarget.Latest, true);
  const literals = [];
  const literalKinds = new Set([
    ts.SyntaxKind.StringLiteral, ts.SyntaxKind.RegularExpressionLiteral,
    ts.SyntaxKind.NoSubstitutionTemplateLiteral, ts.SyntaxKind.TemplateHead,
    ts.SyntaxKind.TemplateMiddle, ts.SyntaxKind.TemplateTail,
  ]);
  const visit = (node) => {
    if (literalKinds.has(node.kind)) literals.push([node.getStart(tree), node.getEnd()]);
    ts.forEachChild(node, visit);
  };
  visit(tree);
  literals.sort(([a], [b]) => a - b);
  let literal = 0;
  return source.replace(/[ \t]+(?=\r?$)/gm, (padding, offset) => {
    while (literal < literals.length && literals[literal][1] <= offset) literal++;
    return literal < literals.length && literals[literal][0] < offset + padding.length ? padding : "";
  });
}

module.exports = { trimSourcePadding };
