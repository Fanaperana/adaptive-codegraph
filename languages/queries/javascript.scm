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

; Object property with function value
(pair
  key: (property_identifier) @symbol.name
  value: [(arrow_function) (function_expression)]) @symbol.def

; Variable declarations (const/let/var with non-function values — constants, configs)
(lexical_declaration
  (variable_declarator
    name: (identifier) @symbol.name
    value: (_))) @symbol.def

; Class fields / properties
(field_definition
  property: (property_identifier) @symbol.name) @symbol.def

; Generator functions
(generator_function_declaration name: (identifier) @symbol.name) @symbol.def

; Export default function/class
(export_statement declaration: (function_declaration name: (identifier) @symbol.name)) @symbol.def
(export_statement declaration: (class_declaration name: (identifier) @symbol.name)) @symbol.def
