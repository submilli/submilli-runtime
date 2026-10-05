---
title: "Filter language"
description: "The language of a permission rule's filter: comparisons, operators, glob and regex patterns, combining conditions, literals, field names, variables, how a filter is evaluated, and the errors a malformed one gives."
slug: reference/filter-language
sidebar:
  order: 2
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "55e3b91757ad4c0b8b51d0a1da57a017584d0e48ca857b87ae9ae6a747147747"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

This page describes the language of the `filter` in a permission rule.
How rules are matched, and the fields each capability reports, are on
[Permissions](/docs/reference/permissions).

## Syntax

A filter is a condition on the fields an operation reports, its
**context**.

```text
amount < 500
customerId == ${vars.customerId} and customerClass == "premium"
not (host glob "*.internal.example.com")
path == "/${vars.userId}" or path glob "/${vars.userId}/*"
```

A comparison is a field name, an operator, and a value, in that order. The
value is a literal or a variable. It is never another field.

## Operators

| Operator | Value | True when |
| --- | --- | --- |
| `==` | String, number, `true`, `false`, `null`, or variable | The field has the value's type and equals it. `field == null` is true only when the field is present and null. |
| `!=` | String, number, `true`, `false`, `null`, or variable | The field has the value's type and differs from it. |
| `<`, `<=`, `>`, `>=` | Number or variable | The field is a number and compares so. |
| `glob` | Quoted pattern, with or without variables | The field is a string and the whole of it fits the pattern. |
| `matches` | Quoted regular expression, without variables | The field is a string and the expression matches part of it. |
| `contains` | String, number, `true`, `false`, or variable | The field is an array and one of its elements equals the value. |

In a `glob` pattern, `*` stands for any run of characters, including none,
`?` for exactly one, and `\` makes the next character literal. `*` crosses
`/`, so `path glob "/notes/*"` matches `/notes/2026/a.md`. Matching is
case-sensitive, and `[` has no special meaning.

`matches` uses the syntax of Rust's `regex` crate, which has no
backreferences or lookaround. It succeeds when the expression matches
anywhere in the field, so `host matches "internal"` matches
`api.internal.example.com`. Write `^` and `$` around the expression to match
the whole field.

## Combining conditions

Conditions combine with `and`, `or`, and `not`. `not` binds tightest, then
`and`, then `or`, and parentheses group. `a or b and not c` reads as
`a or (b and (not c))`.

## Literals

| Literal | Form |
| --- | --- |
| String | Double quotes. `\"`, `\\`, `\n`, `\t`, and `\r` are escapes. Any other `\` is kept with the character after it. |
| Number | Digits with an optional leading `-`, decimal point, and exponent: `500`, `-1`, `2.5`, `1e6`. |
| Boolean | `true`, `false` |
| Null | `null`, with `==` and `!=` only |

## Field names

A field name starts with a letter or `_` and holds letters, digits, and `_`.
A field inside an object is named with a dot, `order.total`, and an element
of an array by its position, `items.0.sku`. `and`, `or`, `not`, `glob`,
`matches`, `contains`, `true`, `false`, and `null` are keywords, never field
names.

## Variables

`${vars.NAME}` stands for the value of a variable the Blueprint declares
under `variables:`. `NAME` holds letters, digits, `_`, and `-`.

- It stands alone as a value, as in `customerId == ${vars.customerId}`, or
  sits inside a quoted string, as in `path glob "/users/${vars.userId}/*"`.
- A variable's value is a string. Standing alone beside a number field it is
  read as a number, and beside a boolean field as `true` or `false`.
- Inside a quoted string it takes the place of its text. With `==`, `!=`,
  and `contains` the result is compared as a string. Inside a `glob` pattern
  the value is taken literally, so a value of `*` matches an asterisk and
  can't widen the pattern.
- `matches` doesn't take variables.
- A session supplies the values when it opens. A variable the session
  doesn't supply, or supplies empty, takes its `default`. Without one it has
  no value.

## How a filter is evaluated

Evaluating a filter never fails. A comparison that can't be decided is
false.

| Situation | Result |
| --- | --- |
| The field is missing from the context | False, for every operator, `!=` included |
| The field's type doesn't suit the operator or the value, such as `<` on a string or `== 5` on a string | False, `!=` included |
| A variable the comparison uses has no value, or can't be read as the field's type | False |
| `not` around a comparison that is false for one of the reasons above | True |

Because of the fourth row, `not (owner == "ops")` is true for a context with
no `owner`, while `owner != "ops"` is false for it.

## Fields

A filter tests the fields of the operation's context. The fields each
capability reports, and their types, are listed under
[Capabilities](/docs/reference/permissions#capabilities). Some fields are
[normalized](/docs/reference/permissions#normalized-fields) before a rule
sees them, and [some are reported by only some calls](/docs/reference/permissions#fields-only-some-calls-report).
`submilli blueprint capability list` prints them for a Blueprint.

## Errors

A filter is parsed when the Blueprint is read, by `submilli blueprint lint`,
by the server when it registers the Blueprint, and by
`submilli run --blueprint`. Each of these errors stops it:

| Mistake | Error |
| --- | --- |
| A malformed filter | ``invalid filter `path = "/a"`: expected `==`; a single `=` is not an operator``, with the filter and a caret under the fault |
| A regular expression that doesn't compile | ``invalid filter `…`: invalid regex:`` and the reason |
| `matches` with a variable | `` `matches` takes a literal regex; `${vars.NAME}` interpolation isn't supported inside a regex pattern `` |
| A variable the Blueprint doesn't declare | `filter references undeclared variable '${vars.customer}'` |

A filter that tests a field the capability doesn't report is an error in
`submilli blueprint lint` and at registration. It is listed with the other
[errors when the Blueprint is read](/docs/reference/permissions#errors-when-the-blueprint-is-read).
