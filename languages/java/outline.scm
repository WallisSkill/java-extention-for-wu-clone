(package_declaration
  "package" @context
  (_) @name) @item

(class_declaration
  (modifiers)? @context
  "class" @context
  name: (_) @name) @item

(interface_declaration
  (modifiers)? @context
  "interface" @context
  name: (_) @name) @item

(enum_declaration
  (modifiers)? @context
  "enum" @context
  name: (_) @name) @item

(record_declaration
  (modifiers)? @context
  "record" @context
  name: (_) @name) @item

(annotation_type_declaration
  (modifiers)? @context
  "@interface" @context
  name: (_) @name) @item

(method_declaration
  (modifiers)? @context
  type: (_) @context
  name: (_) @name
  parameters: (formal_parameters
    "(" @context
    ")" @context)) @item

(constructor_declaration
  (modifiers)? @context
  name: (_) @name
  parameters: (formal_parameters
    "(" @context
    ")" @context)) @item

(field_declaration
  (modifiers)? @context
  type: (_) @context
  declarator: (variable_declarator
    name: (_) @name)) @item

(enum_constant
  name: (_) @name) @item
