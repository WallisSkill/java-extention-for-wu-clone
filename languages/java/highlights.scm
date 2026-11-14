(identifier) @variable

(method_declaration
  name: (identifier) @function.method)
(method_invocation
  name: (identifier) @function.method)
(super) @function.builtin

(annotation
  name: (identifier) @attribute)
(marker_annotation
  name: (identifier) @attribute)
(annotation
  name: (scoped_identifier) @attribute)
(marker_annotation
  name: (scoped_identifier) @attribute)
"@" @attribute

(type_identifier) @type

(interface_declaration
  name: (identifier) @type)
(class_declaration
  name: (identifier) @type)
(enum_declaration
  name: (identifier) @type)
(record_declaration
  name: (identifier) @type)
(annotation_type_declaration
  name: (identifier) @type)

((field_access
  object: (identifier) @type)
 (#match? @type "^[A-Z]"))
((scoped_identifier
  scope: (identifier) @type)
 (#match? @type "^[A-Z]"))
((method_invocation
  object: (identifier) @type)
 (#match? @type "^[A-Z]"))
((method_reference
  . (identifier) @type)
 (#match? @type "^[A-Z]"))

(constructor_declaration
  name: (identifier) @constructor)

(field_declaration
  declarator: (variable_declarator
    name: (identifier) @property))
(field_access
  field: (identifier) @property)

(formal_parameter
  name: (identifier) @variable.parameter)

[
  (boolean_type)
  (integral_type)
  (floating_point_type)
  (void_type)
] @type.builtin

((identifier) @constant
 (#match? @constant "^_*[A-Z][A-Z\\d_]+$"))

(this) @variable.special

[
  (hex_integer_literal)
  (decimal_integer_literal)
  (octal_integer_literal)
  (binary_integer_literal)
  (decimal_floating_point_literal)
  (hex_floating_point_literal)
] @number

[
  (character_literal)
  (string_literal)
] @string
(escape_sequence) @string.escape

[
  (true)
  (false)
  (null_literal)
] @constant.builtin

(line_comment) @comment
(block_comment) @comment
((block_comment) @comment.doc
  (#match? @comment.doc "^/\\*\\*"))

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

[
  ";" "." "," "::"
] @punctuation.delimiter

[
  "=" "+" "-" "*" "/" "%" "!" "~" "<" ">" "<=" ">=" "==" "!=" "&&" "||"
  "&" "|" "^" "<<" ">>" ">>>" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^="
  "<<=" ">>=" ">>>=" "++" "--" "?" ":" "->"
] @operator

[
  "abstract" "assert" "break" "case" "catch" "class" "continue" "default"
  "do" "else" "enum" "exports" "extends" "final" "finally" "for" "if"
  "implements" "import" "instanceof" "interface" "module" "native" "new"
  "non-sealed" "open" "opens" "package" "permits" "private" "protected"
  "provides" "public" "requires" "record" "return" "sealed" "static"
  "strictfp" "switch" "synchronized" "throw" "throws" "to" "transient"
  "transitive" "try" "uses" "volatile" "when" "while" "with" "yield"
] @keyword
