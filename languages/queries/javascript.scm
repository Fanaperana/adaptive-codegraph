; JavaScript symbol extraction queries

; Function declarations (any nesting level)
(function_declaration name: (identifier) @symbol.name) @symbol.def
(generator_function_declaration name: (identifier) @symbol.name) @symbol.def

; Classes and their members
(class_declaration name: (identifier) @symbol.name) @symbol.def
(method_definition name: (property_identifier) @symbol.name) @symbol.def
(field_definition property: (property_identifier) @symbol.name) @symbol.def

; Object literal methods: { handler() {}, onClick: () => {} }
(pair key: (property_identifier) @symbol.name value: [(arrow_function) (function_expression)]) @symbol.def

; Top-level variables (arrow functions / function expressions become kind "function")
(program (lexical_declaration (variable_declarator name: (identifier) @symbol.name) @symbol.def))
(program (variable_declaration (variable_declarator name: (identifier) @symbol.name) @symbol.def))
(export_statement declaration: (lexical_declaration (variable_declarator name: (identifier) @symbol.name) @symbol.def))

; Exported declarations (span includes `export`)
(export_statement declaration: (function_declaration name: (identifier) @symbol.name)) @symbol.def
(export_statement declaration: (class_declaration name: (identifier) @symbol.name)) @symbol.def
