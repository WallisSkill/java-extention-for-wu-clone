(method_declaration
  body: (_
    "{"
    (_)* @function.inside
    "}")) @function.around

(constructor_declaration
  body: (_
    "{"
    (_)* @function.inside
    "}")) @function.around

(class_declaration
  body: (_
    "{"
    (_)* @class.inside
    "}")) @class.around

(interface_declaration
  body: (_
    "{"
    (_)* @class.inside
    "}")) @class.around

(line_comment)+ @comment.around
(block_comment) @comment.around
