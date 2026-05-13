; JavaScript symbol extraction queries

; Function declarations
(function_declaration name: (identifier) @symbol.name) @symbol.def

; Variable declarations with function expressions or arrow functions
(variable_declarator
  name: (identifier) @symbol.name
  value: [(arrow_function) (function_expression)]) @symbol.def

; Class declarations
(class_declaration name: (identifier) @symbol.name) @symbol.def

; Method definitions
(method_definition name: (property_identifier) @symbol.name) @symbol.def

; Object property with function value (e.g. var obj = { myFunc: function() {} })
(pair
  key: (property_identifier) @symbol.name
  value: [(arrow_function) (function_expression)]) @symbol.def
